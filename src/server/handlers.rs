use std::collections::HashMap;
use std::path::PathBuf;
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

use crate::compiler::DefaultCompiler;
use crate::config::manager::ConfigManager;
use crate::config::AppConfig;
use crate::tools::{ToolRegistry, HTTPRequestTool, ShellCommandTool};
use crate::tools::builtin::{CalculatorTool, SearchTool, FileReadTool};
use crate::context::assembler::DefaultContextAssembler;
use crate::executor::DefaultExecutor;
use crate::planner::Planner;
use crate::workflow::WorkflowRegistry;
use crate::providers::ChatProvider;
use crate::requirements::extractor::DefaultRequirementsExtractor;
use crate::resource::DefaultResourceManager;
use crate::scheduler::connector_resolver::ConnectorResolver;
use crate::scheduler::default::DefaultScheduler;
use crate::strategies::chain::ChainStrategy;
use crate::strategies::consensus::ConsensusStrategy;
use crate::strategies::debate::DebateStrategy;
use crate::strategies::fusion::FusionStrategy;
use crate::strategies::react::ReActStrategy;
use crate::strategies::reflection::ReflectionStrategy;
use crate::strategies::single::SingleStrategy;
use crate::strategies::Strategy;
use crate::telemetry::EvidenceRepository;
use tokio_util::sync::CancellationToken;
use crate::resource::cancelling_stream::metered_stream_with_finish;
use crate::resource::guard::ResourceGuard;
use crate::resource::ResourceManager;
use crate::providers::ModelPricing;
use crate::types::*;
use crate::server::pipeline::{
    PipelineStep, PipelineContext, ContextAssemblyStep, RequirementsExtractionStep,
    EvidenceSnapshotStep, PlanningStep, CompilationStep, ResourceReservationStep,
    SchedulingExecutionStep, ResponseBuilderStep,
};

#[derive(Clone)]
pub struct AppState {
    pub context_assembler: Arc<DefaultContextAssembler>,
    pub requirements_extractor: Arc<DefaultRequirementsExtractor>,
    pub planner: Arc<dyn Planner + Send + Sync>,
    pub compiler: Arc<DefaultCompiler>,
    pub scheduler: Arc<DefaultScheduler>,
    pub executor: Arc<DefaultExecutor>,
    pub resource_manager: Arc<DefaultResourceManager>,
    pub evidence_repository: Arc<dyn EvidenceRepository + Send + Sync>,
    pub provider: Arc<dyn ChatProvider + Send + Sync>,
    pub config_manager: Arc<ConfigManager>,
    pub workflow_registry: Arc<WorkflowRegistry>,
    pub tool_registry: Arc<ToolRegistry>,
    pub connector_resolver: Arc<ConnectorResolver>,
}

impl AppState {
    pub fn new(
        provider: Arc<dyn ChatProvider + Send + Sync>,
        resource_manager: DefaultResourceManager,
        evidence_repository: Arc<dyn EvidenceRepository + Send + Sync>,
        config: AppConfig,
        config_path: PathBuf,
        connector_resolver: Arc<ConnectorResolver>,
    ) -> Self {
        let context_assembler = Arc::new(DefaultContextAssembler::new());
        let requirements_extractor = Arc::new(DefaultRequirementsExtractor);

        let mut workflow_registry = WorkflowRegistry::new();
        let _ = workflow_registry.load_dir("workflows");
        let workflow_registry = Arc::new(workflow_registry);

        let planner: Arc<dyn Planner + Send + Sync> = Arc::new(
            crate::planner::IntentPlanner::new(config.model_catalog.clone()),
        );

        let resource_manager = Arc::new(resource_manager);

        // Law 1 / ADR-034: single construction path for the compiler pass pipeline.
        let compiler = Arc::new(crate::compiler::build_compiler(
            config.model_catalog.clone(),
            resource_manager.clone(),
            None,
        ));

        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(StrategyKind::Single, Box::new(SingleStrategy));
        strategies.insert(
            StrategyKind::Consensus,
            Box::new(ConsensusStrategy {
                count: config.strategies.consensus_count,
            }),
        );
        strategies.insert(StrategyKind::Reflection, Box::new(ReflectionStrategy::default()));
        strategies.insert(StrategyKind::Chain, Box::new(ChainStrategy {
            stages: vec![
                Box::new(SingleStrategy),
                Box::new(ReflectionStrategy::default()),
            ],
        }));
        // Build tool registry from config
        let mut tool_registry = ToolRegistry::new();
        tool_registry.register(Arc::new(CalculatorTool));
        tool_registry.register(Arc::new(SearchTool));
        for dir in &config.tools.allowed_read_directories {
            tool_registry.register(Arc::new(FileReadTool::new(dir.clone())));
        }
        if config.tools.enable_http_tool {
            tool_registry.register(Arc::new(HTTPRequestTool::new()));
        }
        tool_registry.register(Arc::new(ShellCommandTool::new(
            config.tools.allowed_shell_commands.clone(),
            config.tools.shell_timeout_secs,
            config.tools.allowed_read_directories.clone(),
            config.tools.allow_unrestricted_args,
        )));
        let tool_registry = Arc::new(tool_registry);

        strategies.insert(StrategyKind::ReAct, Box::new(ReActStrategy::new(
            10,
            Some(tool_registry.clone()),
        )));
        strategies.insert(StrategyKind::Debate, Box::new(DebateStrategy {
            debaters: vec![
                Box::new(SingleStrategy),
                Box::new(SingleStrategy),
            ],
            judge: Box::new(SingleStrategy),
        }));
        strategies.insert(StrategyKind::Fusion, Box::new(FusionStrategy::new(
            vec![
                Box::new(SingleStrategy) as Box<dyn Strategy>,
                Box::new(ConsensusStrategy::default()) as Box<dyn Strategy>,
            ],
        )));

        let executor = Arc::new(DefaultExecutor::new(
            provider.clone(),
            strategies,
        )
        .with_tool_registry(tool_registry.clone())
        .with_allow_auto_exec(config.tools.allow_auto_exec));

        let scheduler = Arc::new(DefaultScheduler::new(
            config.resources.max_concurrent_nodes as usize,
        ));

        let config_manager = Arc::new(ConfigManager::new(
            config_path,
            config,
            vec![],
        ));

        Self {
            context_assembler,
            requirements_extractor,
            planner,
            compiler,
            scheduler,
            executor,
            resource_manager,
            evidence_repository,
            provider,
            config_manager,
            workflow_registry,
            tool_registry,
            connector_resolver,
        }
    }
}

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
            (status, Json(error_response(request_id, &request.model, &e.to_string()))).into_response()
        }
    }
}

/// Builds the admission/reservation graph for a streaming request. Streaming
/// runs directly against the provider (no planner/compiler), so the estimate
/// is derived from what the client already sent. It reserves quota up-front;
/// when the stream terminates the measured usage (stream meter) replaces the
/// estimate via `ResourceManager::record_usage`.
fn stream_graph_estimate(request_id: Uuid, request: &ChatCompletionRequest) -> ExecutionGraph {
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
            // Exact accounting: the admission estimate is released when the
            // stream ends, then the measured tokens/cost are recorded.
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
                        serde_json::json!({"error": e.to_string()})
                    }
                };
                Ok(Event::default().data(serde_json::to_string(&payload).unwrap_or_default()))
            }))
        }
        Err(e) => {
            let error = serde_json::json!({"error": e.to_string()});
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

async fn process_request(
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
    // The caller's explicit model wins over the planner's catalog defaults;
    // otherwise requests for e.g. "openrouter/auto" silently execute on a
    // catalog model the provider may not offer.
    if !request.model.trim().is_empty() {
        for node in &mut ir.nodes {
            node.model = Some(request.model.trim().to_string());
        }
    }

    // Strategy override: the caller may skip the plan's workflow shape and
    // run one ensemble node — e.g. a multi-model consensus where each
    // member (a different model) reviews the same task with its own tool
    // loop and the judge consolidates the member outputs.
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
        let mut config: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();
        if kind == crate::types::StrategyKind::Consensus {
            config.insert("count".into(), serde_json::json!(strategy.count));
            if !strategy.members.is_empty() {
                config.insert(
                    "members".into(),
                    serde_json::json!(strategy.members),
                );
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

    // Record graph compilation rate: the hash is unique per request, so it
    // must never be used as a label (unbounded cardinality would balloon
    // memory and break the metrics scrape).
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
    let result = step_exec.execute((graph.clone(), reservation), &mut pctx).await?;

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

pub async fn metrics_handler() -> impl IntoResponse {
    let metrics = crate::telemetry::metrics::render_metrics();
    (
        StatusCode::OK,
        [("Content-Type", "text/plain; charset=utf-8")],
        metrics,
    )
}

fn error_response(request_id: Uuid, model: &str, error: &str) -> ChatCompletionResponse {
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

            // Set when a chunk error occurs so the footer (which would
            // otherwise report a successful end_turn) is suppressed.
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

            // A failed stream must not be presented as a completed
            // (end_turn) message: the stop footer is emitted only when no
            // error chunk was observed.
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn test_health_endpoint() {
        let res = crate::server::health::health_handler().await;
        assert_eq!(res["status"], "ok");
    }

    #[tokio::test]
    async fn test_ready_endpoint() {
        use crate::config::{
            AppConfig, AuthConfig, CorsConfig, LoggingConfig, RateLimitingConfig,
            ResourceConfig, ServerConfig, StrategyConfig, ToolsConfig,
        };
        let config = AppConfig {
            unsafe_dev: false,
            server: ServerConfig {
                host: "0.0.0.0".into(),
                port: 0,
                shutdown_timeout_secs: 30,
                cors: CorsConfig::default(),
            },
            resources: ResourceConfig {
                max_daily_cost: 100.0,
                max_daily_tokens: 100000,
                max_concurrent: 10,
                max_concurrent_nodes: 16,
                provider_limits: Default::default(),
            },
            policies: vec![],
            providers: Default::default(),
            strategies: StrategyConfig { consensus_count: 3 },
            tools: ToolsConfig::default(),
            auth: AuthConfig::default(),
            rate_limiting: RateLimitingConfig::default(),
            logging: LoggingConfig::default(),
            model_catalog: Default::default(),
            connectors: HashMap::new(),
            features: HashMap::new(),
        };
        let state = AppState::new(
            Arc::new(crate::providers::openrouter::OpenRouterProvider::new(
                "test".into(),
            )),
            crate::resource::DefaultResourceManager::new(config.to_quota()),
            Arc::new(
                crate::telemetry::SqliteEvidenceRepository::new(":memory:").unwrap(),
            ),
            config,
            PathBuf::from("config/default.yaml"),
            Arc::new(ConnectorResolver::new()),
        );
        let (status, res) =
            crate::server::health::ready_handler(axum::extract::State(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(res["status"], "ok");
    }

    #[test]
    fn test_invalid_json_returns_400() {
        let bad_json = r#"{"model": "test"}"#;
        let result: Result<ChatCompletionRequest, _> = serde_json::from_str(bad_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_response_format() {
        let request_id = Uuid::new_v4();
        let response = error_response(request_id, "test-model", "something went wrong");
        assert_eq!(response.model, "test-model");
        assert_eq!(response.choices[0].finish_reason, "error");
        assert!(
            response.choices[0]
                .message
                .content
                .contains("something went wrong")
        );
        assert_eq!(response.object, "chat.completion");
    }

    #[test]
    fn test_anthropic_request_deserialization_and_conversion() {
        let json_body = serde_json::json!({
            "model": "claude-3-5-sonnet-20241022",
            "system": "You are a helpful assistant.",
            "messages": [
                {"role": "user", "content": "Hello Anthropic!"}
            ],
            "max_tokens": 512,
            "temperature": 0.7
        });

        let anthropic_req: AnthropicMessagesRequest = serde_json::from_value(json_body).unwrap();
        assert_eq!(anthropic_req.model, "claude-3-5-sonnet-20241022");

        let chat_req = anthropic_req.into_chat_completion_request();
        assert_eq!(chat_req.model, "claude-3-5-sonnet-20241022");
        assert_eq!(chat_req.messages.len(), 2);
        assert_eq!(chat_req.messages[0].role, "system");
        assert_eq!(chat_req.messages[0].content, "You are a helpful assistant.");
        assert_eq!(chat_req.messages[1].role, "user");
        assert_eq!(chat_req.messages[1].content, "Hello Anthropic!");
        assert_eq!(chat_req.max_tokens, Some(512));
    }

    #[test]
    fn test_anthropic_response_conversion() {
        let completion_resp = ChatCompletionResponse {
            id: "resp-123".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "claude-3-5-sonnet".into(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: "Hi from Anthropic response!".into(),
                },
                finish_reason: "stop".into(),
            }],
            native_tool_calls: None,
            usage: Some(Usage {
                prompt_tokens: 15,
                completion_tokens: 8,
                total_tokens: 23,
            }),
        };

        let anthropic_resp = AnthropicMessagesResponse::from((completion_resp, "claude-3-5-sonnet".to_string()));
        assert_eq!(anthropic_resp.id, "msg_resp-123");
        assert_eq!(anthropic_resp.r#type, "message");
        assert_eq!(anthropic_resp.role, "assistant");
        assert_eq!(anthropic_resp.stop_reason, Some("end_turn".into()));
        assert_eq!(anthropic_resp.usage.input_tokens, 15);
        assert_eq!(anthropic_resp.usage.output_tokens, 8);
    }
}
