use axum::{
    extract::State,
    http::StatusCode,
    response::sse::{Event, Sse},
    response::IntoResponse,
    Json,
};
use futures::stream::{self, BoxStream};
use futures::StreamExt;
use tracing::Instrument;
use uuid::Uuid;

use super::chat::{process_request, ChatOutcome};
use super::state::AppState;
use crate::types::*;

pub async fn anthropic_messages(
    State(state): State<AppState>,
    Json(anthropic_req): Json<AnthropicMessagesRequest>,
) -> impl IntoResponse {
    let request_id = Uuid::new_v4();
    let model_name = anthropic_req.model.clone();
    let is_stream = anthropic_req.stream;
    let span = tracing::info_span!(
        "anthropic_messages",
        request_id = %request_id,
        model = %model_name,
        stream = %is_stream
    );

    if model_name.trim().is_empty() {
        tracing::warn!(request_id = %request_id, "request rejected: empty model");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "type": "error",
                "error": {
                    "type": "invalid_request_error",
                    "message": "model is required"
                }
            })),
        )
            .into_response();
    }

    let request = anthropic_req.into_chat_completion_request();

    // Phase F: all Anthropic requests — streaming and non-streaming — execute
    // through the identical pipeline. Streaming wraps the completed result in
    // Anthropic-format SSE events.
    tracing::info!("processing anthropic request through full pipeline");

    // `Instrument` keeps span context across .await without holding an enter
    // guard over a suspension point.
    let result = process_request(&state, &request, request_id, false)
        .instrument(span)
        .await;

    match result {
        Ok(outcome) => {
            // Native OpenAI-format streams are not re-emitted here; the
            // Anthropic endpoint keeps its own completed-result transport.
            let response = match outcome {
                ChatOutcome::Completed(r) => r,
                ChatOutcome::Stream(_) => unreachable!("native streaming disabled for anthropic"),
            };
            tracing::info!(request_id = %request_id, status = "success", stream = is_stream);
            if is_stream {
                // Phase F: SSE transport adapter for Anthropic format
                anthropic_stream_completed_response(response, &model_name, request_id).await
            } else {
                let anthropic_resp = AnthropicMessagesResponse::from((response, model_name));
                Json(anthropic_resp).into_response()
            }
        }
        Err(e) => {
            // Full detail (Display) stays server-side; clients get the same
            // sanitized message as the OpenAI-compatible endpoint.
            let status = e.status_code();
            tracing::error!(request_id = %request_id, stage = ?e.stage(), error = %e, "anthropic pipeline failed");
            (
                status,
                Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": e.user_message()
                    }
                })),
            )
                .into_response()
        }
    }
}

/// Phase F: SSE transport adapter for Anthropic format — wraps a completed
/// `ChatCompletionResponse` in Anthropic-format SSE events. The pipeline
/// executed identically to non-streaming; only the response encoding differs.
async fn anthropic_stream_completed_response(
    response: ChatCompletionResponse,
    model: &str,
    request_id: Uuid,
) -> axum::response::Response {
    let msg_id = format!("msg_{}", request_id);
    let model_name = model.to_string();

    // Extract content from the completed response
    let content = response
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    // Split content into token-sized chunks for streaming simulation
    let chunk_size = 16;
    let chunks: Vec<String> = content
        .chars()
        .collect::<Vec<_>>()
        .chunks(chunk_size)
        .map(|c| c.iter().collect())
        .collect();

    let total_chunks = chunks.len();

    let stream: BoxStream<'static, Result<Event, std::convert::Infallible>> = Box::pin(
        stream::iter(0..=total_chunks)
            .enumerate()
            .flat_map(move |(i, _)| {
                let msg_id = msg_id.clone();
                let model = model_name.clone();
                let mut events: Vec<Result<Event, std::convert::Infallible>> = Vec::new();

                if i == 0 {
                    // message_start
                    events.push(Ok(Event::default().event("message_start").data(
                        serde_json::to_string(&serde_json::json!({
                            "type": "message_start",
                            "message": {
                                "id": msg_id,
                                "type": "message",
                                "role": "assistant",
                                "model": model,
                                "content": [],
                                "stop_reason": null,
                                "stop_sequence": null,
                                "usage": { "input_tokens": 0, "output_tokens": 0 }
                            }
                        }))
                        .unwrap_or_default(),
                    )));
                    // content_block_start
                    events.push(Ok(Event::default().event("content_block_start").data(
                        serde_json::to_string(&serde_json::json!({
                            "type": "content_block_start",
                            "index": 0,
                            "content_block": { "type": "text", "text": "" }
                        }))
                        .unwrap_or_default(),
                    )));
                    // ping
                    events.push(Ok(Event::default().event("ping").data(
                        serde_json::to_string(&serde_json::json!({ "type": "ping" }))
                            .unwrap_or_default(),
                    )));
                }

                if i < total_chunks {
                    // content_block_delta
                    events.push(Ok(Event::default().event("content_block_delta").data(
                        serde_json::to_string(&serde_json::json!({
                            "type": "content_block_delta",
                            "index": 0,
                            "delta": { "type": "text_delta", "text": chunks[i].clone() }
                        }))
                        .unwrap_or_default(),
                    )));
                }

                if i == total_chunks {
                    // content_block_stop
                    events.push(Ok(Event::default().event("content_block_stop").data(
                        serde_json::to_string(&serde_json::json!({
                            "type": "content_block_stop",
                            "index": 0
                        }))
                        .unwrap_or_default(),
                    )));
                    // message_delta
                    events.push(Ok(Event::default().event("message_delta").data(
                        serde_json::to_string(&serde_json::json!({
                            "type": "message_delta",
                            "delta": { "stop_reason": "end_turn", "stop_sequence": null },
                            "usage": { "output_tokens": 0 }
                        }))
                        .unwrap_or_default(),
                    )));
                    // message_stop
                    events.push(Ok(Event::default().event("message_stop").data(
                        serde_json::to_string(&serde_json::json!({
                            "type": "message_stop"
                        }))
                        .unwrap_or_default(),
                    )));
                }

                stream::iter(events)
            }),
    );

    Sse::new(stream).into_response()
}
