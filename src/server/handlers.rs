use std::collections::HashMap;
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

use crate::compiler::passes::BudgetOptimisationPass;
use crate::compiler::passes::ControlFlowValidationPass;
use crate::compiler::DefaultCompiler;
use crate::compiler::passes::{ConstraintValidationPass, ModelResolutionPass};
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
    pub config: Arc<AppConfig>,
    pub workflow_registry: Arc<WorkflowRegistry>,
    pub tool_registry: Arc<ToolRegistry>,
}

impl AppState {
    pub fn new(
        provider: Arc<dyn ChatProvider + Send + Sync>,
        resource_manager: DefaultResourceManager,
        evidence_repository: Arc<dyn EvidenceRepository + Send + Sync>,
        config: AppConfig,
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

        let compiler = Arc::new(DefaultCompiler {
            passes: vec![
                Box::new(ConstraintValidationPass),
                Box::new(ControlFlowValidationPass),
                Box::new(ModelResolutionPass {
                    model_catalog: config.model_catalog.clone(),
                    model_requirements: None,
                }),
                Box::new(BudgetOptimisationPass {
                    resource_manager: resource_manager.clone(),
                }),
            ],
        });

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
        ).with_tool_registry(tool_registry.clone()));

        let scheduler = Arc::new(DefaultScheduler::new(
            config.resources.max_concurrent_nodes as usize,
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
            config: Arc::new(config),
            workflow_registry,
            tool_registry,
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

async fn stream_response(
    state: AppState,
    request: ChatCompletionRequest,
    request_id: Uuid,
) -> axum::response::Response {
    let request_id_str = request_id.to_string();
    let model_name = request.model.clone();
    let created = chrono::Utc::now().timestamp();

    let provider = state.provider.clone();
    let inner = provider.chat_stream(&request).await;
    drop(request);
    drop(state);

    let stream: BoxStream<'static, Result<Event, std::convert::Infallible>> = match inner {
        Ok(inner_stream) => Box::pin(inner_stream.map(move |chunk_result| {
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
                    serde_json::json!({"error": e.to_string()})
                }
            };
            Ok(Event::default().data(serde_json::to_string(&payload).unwrap_or_default()))
        })),
        Err(e) => {
            let error = serde_json::json!({"error": e.to_string()});
            Box::pin(stream::once(async move {
                Ok(Event::default().data(serde_json::to_string(&error).unwrap_or_default()))
            }))
        }
    };

    let sse = stream.chain(stream::once(async {
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
    let policies = state.config.to_policies();
    let step_plan = PlanningStep {
        planner: state.planner.clone(),
        policies,
    };
    let ir = step_plan.execute((reqs.clone(), evidence), &mut pctx).await?;
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

    // Record graph hash distribution
    let hash_str = format!("{:016x}", graph.primitive_graph_hash);
    crate::telemetry::metrics::FusionMetrics::instance()
        .graph_hash_count
        .with_label_values(&[&hash_str])
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
            cost: result.total_cost as f64,
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
        usage: None,
    }
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
}
