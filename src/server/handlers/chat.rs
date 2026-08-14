use axum::{
    extract::State,
    http::StatusCode,
    response::sse::{Event, Sse},
    response::IntoResponse,
    Json,
};
use futures::stream::{self, BoxStream};
use futures::StreamExt;
use uuid::Uuid;

use super::state::AppState;
use crate::server::pipeline::{
    CompilationStep, ContextAssemblyStep, EvidenceSnapshotStep, PipelineContext, PipelineStep,
    PlanningStep, RequirementsExtractionStep, ResourceReservationStep, ResponseBuilderStep,
    SchedulingExecutionStep,
};
use crate::types::*;

pub async fn chat_completions(
    State(state): State<AppState>,
    Json(request): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    let request_id = Uuid::new_v4();
    let _span = tracing::info_span!(
        "chat_completions",
        request_id = %request_id,
        model = %request.model,
        stream = %request.stream
    );

    let _enter = _span.enter();

    if request.model.trim().is_empty() {
        tracing::warn!(request_id = %request_id, "request rejected: empty model");
        return (
            StatusCode::BAD_REQUEST,
            Json(error_response(request_id, "", "model is required")),
        )
            .into_response();
    }

    // Phase F: all requests — streaming and non-streaming — execute through
    // the identical pipeline (PlanningRequest → fusion-ir → fusion-compiler →
    // fusion-scheduler → fusion-runtime). The only difference is the transport
    // layer: streaming wraps the final result in SSE chunked output.
    tracing::info!("processing request through full pipeline");

    let is_stream = request.stream;
    let result = process_request(&state, &request, request_id).await;

    match result {
        Ok(response) => {
            tracing::info!(request_id = %request_id, status = "success", stream = is_stream);
            if is_stream {
                // Phase F: SSE transport adapter — wrap the completed result in
                // SSE chunks. The pipeline executed identically to non-streaming;
                // only the response encoding differs.
                stream_completed_response(response, &request.model, request_id).await
            } else {
                Json(response).into_response()
            }
        }
        Err(e) => {
            let status = e.status_code();
            tracing::error!(request_id = %request_id, stage = ?e.stage(), error = %e, "pipeline failed");
            (
                status,
                Json(error_response(request_id, &request.model, &e.user_message())),
            )
                .into_response()
        }
    }
}

/// Phase F: SSE transport adapter — wraps a completed `ChatCompletionResponse`
/// in SSE chunked output. The pipeline executed identically to non-streaming;
/// only the response encoding differs. This ensures Gate 08 (Streaming
/// Authority): streaming and non-streaming share the same `ExecutionGraph`.
pub(crate) async fn stream_completed_response(
    response: ChatCompletionResponse,
    model: &str,
    request_id: Uuid,
) -> axum::response::Response {
    let id = response.id.clone();
    let model_name = model.to_string();
    let created = response.created;

    // Extract content from the completed response
    let content = response
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();
    let finish_reason = response
        .choices
        .first()
        .map(|c| c.finish_reason.clone())
        .unwrap_or_else(|| "stop".to_string());

    // Split content into token-sized chunks for streaming simulation
    let chunk_size = 16;
    let chunks: Vec<String> = content
        .chars()
        .collect::<Vec<_>>()
        .chunks(chunk_size)
        .map(|c| c.iter().collect())
        .collect();

    let total_chunks = chunks.len();

    let event_stream: BoxStream<'static, Result<Event, std::convert::Infallible>> =
        Box::pin(stream::iter(0..=total_chunks).enumerate().map(move |(i, _)| {
            let id = id.clone();
            let model = model_name.clone();
            let payload = if i < total_chunks {
                serde_json::json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": chunks[i].clone(),
                        },
                        "finish_reason": null,
                    }],
                })
            } else {
                // Final chunk with finish_reason
                serde_json::json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": finish_reason,
                    }],
                })
            };
            Ok(Event::default().data(
                serde_json::to_string(&payload).unwrap_or_default(),
            ))
        }));

    let sse = event_stream.chain(stream::once(async {
        Ok::<_, std::convert::Infallible>(Event::default().data("[DONE]"))
    }));

    Sse::new(sse).into_response()
}

pub(crate) async fn process_request(
    state: &AppState,
    request: &ChatCompletionRequest,
    request_id: Uuid,
) -> Result<ChatCompletionResponse, RouterError> {
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let mut pctx = PipelineContext::new(request_id, request.clone(), cancellation_token);

    tracing::info!(
        request_id = %request_id,
        model = %request.model,
        message_count = request.messages.len(),
        "request received"
    );

    // 1. Context Assembly
    let step_ctx = ContextAssemblyStep {
        assembler: state.context_assembler.clone(),
    };
    let context_snapshot = step_ctx.execute(request.clone(), &mut pctx).await?;
    tracing::debug!(messages = context_snapshot.messages.len(), request_id = %request_id, "context assembled");

    // 2. Requirements Extraction
    let step_reqs = RequirementsExtractionStep {
        extractor: state.requirements_extractor.clone(),
    };
    let reqs = step_reqs.execute(context_snapshot, &mut pctx).await?;
    tracing::debug!(intent = ?reqs.intent_classification, complexity = ?reqs.complexity, request_id = %request_id, "requirements extracted");

    // 3. Evidence Snapshot
    let step_evidence = EvidenceSnapshotStep {
        repository: state.evidence_repository.clone(),
    };
    let evidence = step_evidence.execute((), &mut pctx).await?;

    // 4. Planning
    let snapshot = state.config_manager.snapshot();
    let policies = snapshot.config.to_policies();
    let step_plan = PlanningStep {
        planner: state.planner.clone(),
        policies,
    };
    let mut ir = step_plan
        .execute((reqs.clone(), Some(evidence)), &mut pctx)
        .await?;

    if let Some(strategy) = &request.strategy {
        let kind = match strategy.kind.as_str() {
            "Single" | "single" => crate::types::StrategyKind::Single,
            "Consensus" | "consensus" => crate::types::StrategyKind::Consensus,
            "Reflection" | "reflection" => crate::types::StrategyKind::Reflection,
            "Debate" | "debate" => crate::types::StrategyKind::Debate,
            "ReAct" | "react" => crate::types::StrategyKind::ReAct,
            "Chain" | "chain" => crate::types::StrategyKind::Chain,
            "Fusion" | "fusion" => crate::types::StrategyKind::Fusion,
            other => crate::types::StrategyKind::Custom(other.to_string()),
        };
        let mut config: std::collections::HashMap<String, serde_json::Value> =
            std::collections::HashMap::new();
        if kind == crate::types::StrategyKind::Consensus {
            config.insert("count".into(), serde_json::json!(strategy.count));
            if !strategy.members.is_empty() {
                config.insert("members".into(), serde_json::json!(strategy.members));
            }
        }
        config.insert(
            "max_tool_rounds".into(),
            serde_json::json!(strategy.max_tool_rounds),
        );
        tracing::info!(
            request_id = %request_id,
            strategy = %strategy.kind,
            count = strategy.count,
            members = ?strategy.members,
            "request strategy override applied"
        );
        ir.nodes = vec![crate::types::IRNode {
            id: Uuid::new_v4(),
            kind: crate::types::IRNodeKind::Generate,
            strategy: kind,
            model: Some(request.model.clone()),
            config,
        }];
    }
    tracing::debug!(plan_id = %ir.plan_id, nodes = ir.nodes.len(), request_id = %request_id, "plan created");

    // 5. Compilation
    let step_compile = CompilationStep {
        compiler: state.compiler.clone(),
    };
    let graph = step_compile.execute(ir, &mut pctx).await?;
    tracing::info!(
        request_id = %request_id,
        graph_id = %graph.graph_id,
        node_count = graph.nodes.len(),
        estimated_cost = %graph.metadata.estimated_cost,
        estimated_tokens = graph.metadata.estimated_tokens,
        "graph compiled"
    );

    crate::telemetry::metrics::FusionMetrics::instance()
        .graph_hash_count
        .inc();

    // 6. Resource Reservation with RAII Guard
    let step_reserve = ResourceReservationStep {
        resource_manager: state.resource_manager.clone(),
    };
    let mut guard = step_reserve.execute(graph.clone(), &mut pctx).await?;

    // 7. Scheduling & Execution
    let reservation = ReservationId(Uuid::new_v4());
    let step_exec = SchedulingExecutionStep {
        scheduler: state.scheduler.clone(),
        executor: state.executor.clone(),
    };
    let result = step_exec
        .execute((graph.clone(), reservation), &mut pctx)
        .await?;

    tracing::info!(
        request_id = %request_id,
        instance_id = %result.instance_id,
        success = result.success,
        latency_ms = result.total_latency_ms,
        tokens = result.total_tokens,
        "execution complete"
    );

    // 8. Telemetry Recording
    if result.success {
        let record = ExecutionRecord {
            record_id: Uuid::new_v4(),
            plan_id: result.instance_id,
            node_id: Uuid::nil(),
            model: request.model.clone(),
            provider: state.provider.name().to_string(),
            intent: reqs.intent_classification,
            latency_ms: result.total_latency_ms,
            tokens: result.total_tokens as u32,
            cost: result.total_cost,
            success: result.success,
            timestamp: chrono::Utc::now().timestamp(),
        };
        let _ = state.evidence_repository.record(record).await;
    }

    // 9. Response Building
    let response = ResponseBuilderStep.execute(result, &mut pctx).await?;

    // Commit RAII ResourceGuard on successful completion
    guard.commit();

    Ok(response)
}

pub(crate) fn error_response(
    request_id: Uuid,
    model: &str,
    error: &str,
) -> ChatCompletionResponse {
    ChatCompletionResponse {
        id: request_id.to_string(),
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp(),
        model: model.to_string(),
        choices: vec![Choice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: format!("Error: {}", error),
            },
            finish_reason: "error".to_string(),
        }],
        native_tool_calls: None,
        usage: None,
    }
}
