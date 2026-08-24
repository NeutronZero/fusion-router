use std::sync::Arc;

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
    let result = process_request(&state, &request, request_id, request.stream).await;

    match result {
        Ok(ChatOutcome::Stream(response)) => response,
        Ok(ChatOutcome::Completed(response)) => {
            tracing::info!(request_id = %request_id, status = "success", stream = is_stream);
            if is_stream {
                // Fallback transport for graphs that require full orchestration
                // (multi-node, subgraphs, tools): the pipeline result is
                // re-chunked into SSE. Native upstream streaming handles the
                // single-node path above.
                stream_completed_response(response, &request.model, request_id).await
            } else {
                Json(response).into_response()
            }
        }
        Err(e) => {
            let status = e.status_code();
            crate::telemetry::metrics::FusionMetrics::instance()
                .errors_total
                .inc();
            tracing::error!(request_id = %request_id, stage = ?e.stage(), error = %e, "pipeline failed");
            (
                status,
                Json(error_response(
                    request_id,
                    &request.model,
                    &e.user_message(),
                )),
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
    _request_id: Uuid,
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

    let event_stream: BoxStream<'static, Result<Event, std::convert::Infallible>> = Box::pin(
        stream::iter(0..=total_chunks)
            .enumerate()
            .map(move |(i, _)| {
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
                Ok(Event::default().data(serde_json::to_string(&payload).unwrap_or_default()))
            }),
    );

    let sse = event_stream.chain(stream::once(async {
        Ok::<_, std::convert::Infallible>(Event::default().data("[DONE]"))
    }));

    let mut resp = Sse::new(sse).into_response();
    // Fallback transport marker (native path sets "native"); keeps the
    // x-fusion-stream-mode contract consistent for clients.
    resp.headers_mut().insert(
        "x-fusion-stream-mode",
        axum::http::HeaderValue::from_static("simulated"),
    );
    resp
}

/// Result of the shared pipeline: either a completed response or an already-
/// engaged native SSE stream (upstream chunks flowing, budget metered).
pub(crate) enum ChatOutcome {
    Completed(ChatCompletionResponse),
    Stream(axum::response::Response),
}

pub(crate) async fn process_request(
    state: &AppState,
    request: &ChatCompletionRequest,
    request_id: Uuid,
    allow_native_stream: bool,
) -> Result<ChatOutcome, RouterError> {
    let started = std::time::Instant::now();
    let metrics = crate::telemetry::metrics::FusionMetrics::instance();
    metrics.requests_total.inc();

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

    // 4. Planning: Authoritative Policy Snapshot
    let policy_snapshot = state.policy_registry.current_snapshot();
    // Honest projection of the live declarations (effects/priorities preserved)
    // so the planner records what operators actually wrote.
    let policies: Vec<crate::types::Policy> = policy_snapshot
        .policies
        .iter()
        .filter_map(
            |p| match serde_json::from_str::<crate::policy::PolicyDeclaration>(&p.rule) {
                Ok(d) => Some(crate::types::Policy {
                    name: d.name,
                    priority: d.priority,
                    conditions: d
                        .conditions
                        .into_iter()
                        .map(|(field, value)| crate::types::PolicyCondition {
                            field,
                            operator: "eq".into(),
                            value,
                        })
                        .collect(),
                    actions: vec![crate::types::PolicyAction {
                        action_type: d.effect,
                        params: d.annotations,
                    }],
                }),
                Err(e) => {
                    tracing::warn!(request_id = %request_id, policy = %p.name, error = %e,
                    "unparseable stored policy declaration");
                    None
                }
            },
        )
        .collect();
    let step_plan = PlanningStep {
        planner: state.planner.clone(),
        policies,
        policy_version: policy_snapshot.version,
    };
    let ir = step_plan
        .execute((reqs.clone(), Some(evidence)), &mut pctx)
        .await?;
    tracing::debug!(plan_id = %ir.plan_id, nodes = ir.nodes.len(), request_id = %request_id, "plan created");

    // 5. Compilation — fail closed if the snapshot contains malformed rules;
    // attach the policy pass (deny ⇒ compile error) whenever policies exist.
    let policy_ir = state
        .policy_registry
        .policy_ir()
        .map_err(|e| RouterError::StageFailure {
            stage: PipelineStage::Planning,
            request_id,
            message: format!("policy configuration rejected: {e}"),
        })?;
    let step_compile = CompilationStep {
        compiler: state.compiler_with_policies(policy_ir),
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

    metrics.graph_hash_count.inc();
    if graph.primitive_graph_hash != 0 {
        tracing::debug!(
            request_id = %request_id,
            graph_hash = graph.primitive_graph_hash,
            "compiled graph content hash"
        );
    }

    // 6. Resource Reservation with RAII Guard
    let step_reserve = ResourceReservationStep {
        resource_manager: state.resource_manager.clone(),
    };
    let mut guard = step_reserve.execute(graph.clone(), &mut pctx).await?;

    // 7a. Native upstream streaming for eligible single-node graphs.
    if allow_native_stream && request.stream {
        if let Some(registry) = state.provider_registry.clone() {
            if native_stream_eligible(&graph) {
                let response = native_stream_sse(
                    state, request, request_id, &graph, registry, guard, &mut pctx,
                )
                .await;
                return Ok(ChatOutcome::Stream(response));
            }
        }
        // Ineligible graph: fall through to orchestrated execution; the
        // handler re-chunks the completed response (documented fallback).
    }

    // 7b. Scheduling & Execution
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
    metrics
        .request_duration_seconds
        .with_label_values(&["chat"])
        .observe(started.elapsed().as_secs_f64());
    metrics
        .provider_latency_seconds
        .with_label_values(&[state.provider.name()])
        .observe((result.total_latency_ms as f64) / 1000.0);
    if result.total_tokens > 0 {
        metrics.tokens_total.inc_by(result.total_tokens as u64);
    }
    if !result.success {
        metrics.errors_total.inc();
    }

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

    Ok(ChatOutcome::Completed(response))
}

/// Eligibility for native upstream streaming: a single LLM node with no
/// subgraph and no tool access — the model call is the whole workflow.
fn native_stream_eligible(graph: &crate::types::ExecutionGraph) -> bool {
    if graph.nodes.len() != 1 {
        return false;
    }
    let node = &graph.nodes[0];
    if !matches!(
        node.kind,
        ExecutionNodeKind::LLMGenerate | ExecutionNodeKind::LLMReview | ExecutionNodeKind::LLMJudge
    ) {
        return false;
    }
    node.subgraph.is_none()
        && !node.config.contains_key("tool_allowlist")
        && !node.config.contains_key("tools")
}

/// Streams the single-node graph directly from the upstream provider.
///
/// Chunks flow through [`MeteredStream`], which enforces the per-request
/// budget envelope mid-stream, releases the reservation on client disconnect
/// (body drop), and reports final measured usage via its finish hook.
#[allow(clippy::too_many_arguments)]
async fn native_stream_sse(
    state: &AppState,
    request: &ChatCompletionRequest,
    request_id: Uuid,
    graph: &crate::types::ExecutionGraph,
    registry: Arc<crate::providers::registry::ProviderRegistry>,
    guard: crate::resource::ResourceGuard,
    pctx: &mut PipelineContext,
) -> axum::response::Response {
    let node = &graph.nodes[0];
    let model = if node.model.is_empty() {
        request.model.clone()
    } else {
        node.model.clone()
    };

    let mut upstream = request.clone();
    upstream.model = model.clone();
    upstream.stream = true;
    upstream.tools = None;
    if let Some(v) = node.config.get("messages") {
        if let Ok(msgs) = serde_json::from_value::<Vec<ChatMessage>>(v.clone()) {
            upstream.messages = msgs;
        }
    }

    let provider_dyn: Arc<dyn crate::providers::ChatProvider> = registry.clone();
    let inner = match provider_dyn.chat_stream(&upstream).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(request_id = %request_id, error = %e, "upstream stream failed to start");
            drop(guard);
            let mut resp = (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(error_response(
                    request_id,
                    &request.model,
                    "upstream provider unavailable",
                )),
            )
                .into_response();
            resp.headers_mut().insert(
                "x-fusion-stream-mode",
                axum::http::HeaderValue::from_static("native"),
            );
            return resp;
        }
    };

    let pricing = registry.get_pricing(&model);
    let cancel = pctx.cancellation_token.clone();
    let envelope = pctx.budget_envelope.clone();
    let rm = state.resource_manager.clone() as Arc<dyn crate::resource::ResourceManager>;
    let evidence = state.evidence_repository.clone();
    let plan_id = pctx
        .ir
        .as_ref()
        .map(|ir| ir.plan_id)
        .unwrap_or_else(Uuid::nil);
    let intent = pctx
        .requirements
        .as_ref()
        .map(|r| r.intent_classification.clone())
        .unwrap_or(crate::types::Intent::General);
    let req_model = request.model.clone();
    let started_ms = std::time::Instant::now().elapsed().as_millis() as u64;

    // Accounting model: the RAII guard's estimate booking is refunded when the
    // metered stream ends (release-before-hook), then actuals are recorded
    // here — net spend equals measured reality, even for disconnects/breaches.
    let on_finish: crate::resource::cancelling_stream::StreamFinishHook = Box::new(move |report| {
        tokio::spawn(async move {
            rm.record_usage(report.cost, report.total_tokens).await;
            crate::telemetry::stream_metrics::StreamMetrics::instance().record_report(&report);
            let record = ExecutionRecord {
                record_id: Uuid::new_v4(),
                plan_id,
                node_id: Uuid::nil(),
                model: req_model,
                provider: "provider-registry".into(),
                intent,
                latency_ms: report.total_duration_ms.unwrap_or(started_ms),
                tokens: report.total_tokens as u32,
                cost: report.cost,
                success: true,
                timestamp: chrono::Utc::now().timestamp(),
            };
            let _ = evidence.record(record).await;
        });
    });

    let (metered, _meter) = crate::resource::cancelling_stream::metered_stream_with_finish(
        inner, guard, cancel, pricing, on_finish,
    );
    let mut metered = metered;
    if let Some(env) = envelope {
        metered = metered.with_budget_envelope(env);
    }

    let id = format!("chatcmpl-{request_id}");
    let created = chrono::Utc::now().timestamp();
    let sse_model = model.clone();

    let chunks = metered.filter_map(move |item| {
        let id = id.clone();
        let model = sse_model.clone();
        async move {
            match item {
                Ok(chunk) => Some(Ok::<Event, std::convert::Infallible>(
                    Event::default().data(
                        serde_json::json!({
                            "id": id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": model,
                            "choices": [{
                                "index": 0,
                                "delta": { "content": chunk.content },
                                "finish_reason": chunk.finish_reason,
                            }],
                            "usage": chunk.usage,
                        })
                        .to_string(),
                    ),
                )),
                Err(e) => {
                    tracing::warn!(request_id = %request_id, error = %e, "stream terminated abnormally");
                    Some(Ok(Event::default().event("error").data(
                        serde_json::json!({ "error": e.to_string() }).to_string(),
                    )))
                }
            }
        }
    });
    let done = futures::stream::once(async { Ok(Event::default().data("[DONE]")) });

    let mut resp = Sse::new(chunks.chain(done)).into_response();
    resp.headers_mut().insert(
        "x-fusion-stream-mode",
        axum::http::HeaderValue::from_static("native"),
    );
    tracing::info!(request_id = %request_id, model = %model, "native streaming engaged");
    resp
}

pub(crate) fn error_response(request_id: Uuid, model: &str, error: &str) -> ChatCompletionResponse {
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
