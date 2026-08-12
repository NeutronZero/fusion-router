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

use super::state::AppState;
use crate::providers::ModelPricing;
use crate::resource::cancelling_stream::metered_stream_with_finish;
use crate::resource::guard::ResourceGuard;
use crate::resource::ResourceManager;
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

    if request.stream {
        tracing::info!(request_id = %request_id, "streaming request");
        return stream_response(state, request, request_id).await;
    }

    tracing::info!("processing request through full pipeline");

    let result = process_request(&state, &request, request_id).await;

    match result {
        Ok(response) => {
            tracing::info!(request_id = %request_id, status = "success");
            Json(response).into_response()
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

pub(crate) fn stream_graph_estimate(
    request_id: Uuid,
    request: &ChatCompletionRequest,
) -> ExecutionGraph {
    let input_tokens: u64 = request
        .messages
        .iter()
        .map(|m| (m.content.len() / 4).max(1) as u64)
        .sum();
    let max_output = request.max_tokens.unwrap_or(4096) as u64;
    let estimated_tokens = (input_tokens + max_output).max(1);
    ExecutionGraph {
        graph_id: request_id,
        nodes: vec![],
        edges: vec![],
        metadata: GraphMetadata {
            estimated_cost: 0.0,
            estimated_tokens,
            max_depth: 0,
            node_count: 0,
        },
        total_tokens: estimated_tokens,
        total_cost: 0,
        primitive_graph_hash: 0,
    }
}

async fn stream_response(
    state: AppState,
    request: ChatCompletionRequest,
    request_id: Uuid,
) -> axum::response::Response {
    let request_id_str = request_id.to_string();
    let model_name = request.model.clone();
    let created = chrono::Utc::now().timestamp();

    let pricing: Option<ModelPricing> = None;
    let resource_manager = state.resource_manager.clone();

    let provider = state.provider.clone();
    let inner = provider.chat_stream(&request).await;
    let graph = stream_graph_estimate(request_id, &request);
    drop(request);
    drop(state);

    let event_stream: BoxStream<'static, Result<Event, std::convert::Infallible>> = match inner {
        Ok(inner_stream) => {
            if !resource_manager.try_reserve(&graph).await {
                tracing::warn!(
                    request_id = %request_id,
                    "streaming request rejected: daily resource quota exhausted"
                );
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(error_response(
                        request_id,
                        &model_name,
                        "Daily resource quota exhausted",
                    )),
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
            Box::pin(metered.map(move |chunk_result| {
                let id = request_id_str.clone();
                let model = model_name.clone();
                let payload = match chunk_result {
                    Ok(chunk) => {
                        serde_json::json!({
                            "id": id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": model,
                            "choices": [{
                                "index": 0,
                                "delta": {
                                    "content": chunk.content,
                                },
                                "finish_reason": chunk.finish_reason,
                            }],
                        })
                    }
                    Err(e) => {
                        crate::telemetry::stream_metrics::StreamMetrics::instance()
                            .record_error();
                        tracing::error!(request_id = %request_id_str, error = %e, "stream error mid-response");
                        serde_json::json!({"error": "streaming error"})
                    }
                };
                Ok(Event::default().data(serde_json::to_string(&payload).unwrap_or_default()))
            }))
        }
        Err(e) => {
            tracing::error!(request_id = %request_id, error = %e, "failed to open provider stream");
            let error = serde_json::json!({"error": "upstream provider unavailable"});
            Box::pin(stream::once(async move {
                Ok(Event::default().data(serde_json::to_string(&error).unwrap_or_default()))
            }))
        }
    };

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
    if !request.model.trim().is_empty() {
        for node in &mut ir.nodes {
            node.model = Some(request.model.trim().to_string());
        }
    }

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
        estimated_cost = graph.metadata.estimated_cost,
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
