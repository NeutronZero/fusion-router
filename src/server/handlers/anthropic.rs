use axum::{
    extract::State,
    http::StatusCode,
    response::sse::{Event, Sse},
    response::IntoResponse,
    Json,
};
use futures::stream::{self, BoxStream};
use futures::StreamExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::chat::{process_request, stream_graph_estimate};
use super::state::AppState;
use crate::providers::ModelPricing;
use crate::resource::cancelling_stream::metered_stream_with_finish;
use crate::resource::guard::ResourceGuard;
use crate::resource::ResourceManager;
use crate::types::*;

pub async fn anthropic_messages(
    State(state): State<AppState>,
    Json(anthropic_req): Json<AnthropicMessagesRequest>,
) -> impl IntoResponse {
    let request_id = Uuid::new_v4();
    let model_name = anthropic_req.model.clone();
    let is_stream = anthropic_req.stream;

    let _span = tracing::info_span!(
        "anthropic_messages",
        request_id = %request_id,
        model = %model_name,
        stream = %is_stream
    );
    let _enter = _span.enter();

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

    if is_stream {
        tracing::info!(request_id = %request_id, "anthropic streaming request");
        return anthropic_stream_response(state, request, request_id, model_name).await;
    }

    tracing::info!("processing anthropic request through full pipeline");

    let result = process_request(&state, &request, request_id).await;

    match result {
        Ok(response) => {
            tracing::info!(request_id = %request_id, status = "success");
            let anthropic_resp = AnthropicMessagesResponse::from((response, model_name));
            Json(anthropic_resp).into_response()
        }
        Err(e) => {
            let status = e.status_code();
            tracing::error!(request_id = %request_id, stage = ?e.stage(), error = %e, "anthropic pipeline failed");
            (
                status,
                Json(serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": e.to_string()
                    }
                })),
            )
                .into_response()
        }
    }
}

async fn anthropic_stream_response(
    state: AppState,
    request: ChatCompletionRequest,
    request_id: Uuid,
    model_name: String,
) -> axum::response::Response {
    let msg_id = format!("msg_{}", request_id);
    let pricing: Option<ModelPricing> = None;
    let resource_manager = state.resource_manager.clone();
    let provider = state.provider.clone();
    let inner = provider.chat_stream(&request).await;
    let graph = stream_graph_estimate(request_id, &request);
    drop(request);
    drop(state);

    let stream: BoxStream<'static, Result<Event, std::convert::Infallible>> = match inner {
        Ok(inner_stream) => {
            if !resource_manager.try_reserve(&graph).await {
                tracing::warn!(
                    request_id = %request_id,
                    "anthropic streaming request rejected: daily resource quota exhausted"
                );
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(serde_json::json!({
                        "type": "error",
                        "error": {
                            "type": "resource_exhausted",
                            "message": "Daily resource quota exhausted"
                        }
                    })),
                )
                    .into_response();
            }
            let cancel = CancellationToken::new();
            let guard = ResourceGuard::new(request_id, graph, resource_manager.clone());
            let hook_manager = resource_manager.clone();
            let (metered, _meter) = metered_stream_with_finish(
                inner_stream,
                guard,
                cancel,
                pricing,
                Box::new(move |report| {
                    let manager = hook_manager.clone();
                    tokio::spawn(async move {
                        manager
                            .record_usage(report.cost_millicosts, report.total_tokens)
                            .await;
                    });
                    crate::telemetry::stream_metrics::StreamMetrics::instance()
                        .record_report(&report);
                }),
            );

            let id_clone = msg_id.clone();
            let model_clone = model_name.clone();

            let message_start = Event::default()
                .event("message_start")
                .data(serde_json::to_string(&serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": id_clone,
                        "type": "message",
                        "role": "assistant",
                        "model": model_clone,
                        "content": [],
                        "stop_reason": null,
                        "stop_sequence": null,
                        "usage": { "input_tokens": 0, "output_tokens": 0 }
                    }
                })).unwrap_or_default());

            let content_block_start = Event::default()
                .event("content_block_start")
                .data(serde_json::to_string(&serde_json::json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": { "type": "text", "text": "" }
                })).unwrap_or_default());

            let ping = Event::default()
                .event("ping")
                .data(serde_json::to_string(&serde_json::json!({ "type": "ping" })).unwrap_or_default());

            let header_stream = stream::iter(vec![Ok(message_start), Ok(content_block_start), Ok(ping)]);

            let stream_failed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let footer_failed = stream_failed.clone();

            let delta_stream = metered.map(move |chunk_result| {
                match chunk_result {
                    Ok(chunk) => {
                        let text = chunk.content.unwrap_or_default();
                        Ok(Event::default()
                            .event("content_block_delta")
                            .data(serde_json::to_string(&serde_json::json!({
                                "type": "content_block_delta",
                                "index": 0,
                                "delta": { "type": "text_delta", "text": text }
                            })).unwrap_or_default()))
                    }
                    Err(e) => {
                        stream_failed.store(true, std::sync::atomic::Ordering::SeqCst);
                        crate::telemetry::stream_metrics::StreamMetrics::instance()
                            .record_error();
                        Ok(Event::default()
                            .event("error")
                            .data(serde_json::to_string(&serde_json::json!({
                                "type": "error",
                                "error": { "type": "api_error", "message": e.to_string() }
                            })).unwrap_or_default()))
                    }
                }
            });

            let content_block_stop = Event::default()
                .event("content_block_stop")
                .data(serde_json::to_string(&serde_json::json!({
                    "type": "content_block_stop",
                    "index": 0
                })).unwrap_or_default());

            let message_delta = Event::default()
                .event("message_delta")
                .data(serde_json::to_string(&serde_json::json!({
                    "type": "message_delta",
                    "delta": { "stop_reason": "end_turn", "stop_sequence": null },
                    "usage": { "output_tokens": 0 }
                })).unwrap_or_default());

            let message_stop = Event::default()
                .event("message_stop")
                .data(serde_json::to_string(&serde_json::json!({
                    "type": "message_stop"
                })).unwrap_or_default());

            let footer_stream = stream::iter(vec![
                Ok(content_block_stop),
                Ok(message_delta),
                Ok(message_stop),
            ])
            .filter_map(move |evt| {
                let failed = footer_failed.clone();
                async move {
                    if failed.load(std::sync::atomic::Ordering::SeqCst) {
                        None
                    } else {
                        Some(evt)
                    }
                }
            });

            Box::pin(header_stream.chain(delta_stream).chain(footer_stream))
        }
        Err(e) => {
            let error_evt = Event::default()
                .event("error")
                .data(serde_json::to_string(&serde_json::json!({
                    "type": "error",
                    "error": { "type": "api_error", "message": e.to_string() }
                })).unwrap_or_default());
            Box::pin(stream::once(async move { Ok(error_evt) }))
        }
    };

    Sse::new(stream).into_response()
}
