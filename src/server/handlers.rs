use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use uuid::Uuid;

use crate::compiler::passes::BudgetOptimisationPass;
use crate::compiler::passes::ControlFlowValidationPass;
use crate::compiler::Compiler;
use crate::compiler::DefaultCompiler;
use crate::compiler::passes::{ConstraintValidationPass, ModelResolutionPass};
use crate::config::AppConfig;
use crate::context::assembler::ContextAssembler;
use crate::tools::{ToolRegistry, HTTPRequestTool, ShellCommandTool};
use crate::tools::builtin::{CalculatorTool, SearchTool, FileReadTool};
use crate::context::assembler::DefaultContextAssembler;
use crate::executor::DefaultExecutor;
use crate::planner::Planner;
use crate::planner::WorkflowPlanner;
use crate::workflow::WorkflowRegistry;
use crate::providers::ChatProvider;
use crate::requirements::extractor::RequirementsExtractor;
use crate::requirements::extractor::DefaultRequirementsExtractor;
use crate::resource::ResourceManager;
use crate::resource::DefaultResourceManager;
use crate::scheduler::Scheduler;
use crate::scheduler::default::DefaultScheduler;
use crate::strategies::chain::ChainStrategy;
use crate::strategies::consensus::ConsensusStrategy;
use crate::strategies::debate::DebateStrategy;
use crate::strategies::react::ReActStrategy;
use crate::strategies::reflection::ReflectionStrategy;
use crate::strategies::single::SingleStrategy;
use crate::strategies::Strategy;
use crate::telemetry::EvidenceRepository;
use crate::types::*;

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
            WorkflowPlanner::new(workflow_registry.clone()),
        );

        let resource_manager = Arc::new(resource_manager);

        let compiler = Arc::new(DefaultCompiler {
            passes: vec![
                Box::new(ConstraintValidationPass),
                Box::new(ControlFlowValidationPass),
                Box::new(ModelResolutionPass),
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
        strategies.insert(StrategyKind::Reflection, Box::new(ReflectionStrategy));
        strategies.insert(StrategyKind::Chain, Box::new(ChainStrategy {
            stages: vec![
                Box::new(SingleStrategy),
                Box::new(ReflectionStrategy),
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

        let executor = Arc::new(DefaultExecutor::new(
            provider.clone(),
            strategies,
        ).with_tool_registry(tool_registry.clone()));

        let scheduler = Arc::new(DefaultScheduler::new());

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
        return stream_response(state, request, request_id).into_response();
    }

    tracing::info!("processing request through full pipeline");

    let result = process_request(&state, &request, request_id).await;

    match result {
        Ok(response) => {
            tracing::info!(request_id = %request_id, status = "success");
            Json(response).into_response()
        }
        Err(e) => {
            tracing::error!(request_id = %request_id, error = %e, "pipeline failed");
            Json(error_response(request_id, &request.model, &e.to_string())).into_response()
        }
    }
}

fn stream_response(
    _state: AppState,
    request: ChatCompletionRequest,
    request_id: Uuid,
) -> axum::response::Response {
    let chunk = serde_json::json!({
        "id": request_id,
        "object": "chat.completion.chunk",
        "created": chrono::Utc::now().timestamp(),
        "model": request.model,
        "choices": [{
            "index": 0,
            "delta": {"role": "assistant", "content": "Hello"},
            "finish_reason": null
        }]
    });
    let body = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        chunk.to_string()
    );
    (
        StatusCode::OK,
        [("content-type", "text/event-stream")],
        body,
    )
        .into_response()
}

async fn process_request(
    state: &AppState,
    request: &ChatCompletionRequest,
    request_id: Uuid,
) -> anyhow::Result<ChatCompletionResponse> {
    // 1. Assemble context
    let ctx = state.context_assembler.assemble(request).await?;
    tracing::debug!(messages = ctx.messages.len(), "context assembled");

    // 2. Extract requirements
    let reqs = state.requirements_extractor.extract(&ctx);
    tracing::debug!(intent = ?reqs.intent_classification, complexity = ?reqs.complexity, "requirements extracted");

    // 3. Get evidence snapshot
    let evidence = state.evidence_repository.snapshot().await.ok();

    // 4. Plan
    let policies = state.config.to_policies();
    let ir = state.planner.plan(&reqs, &policies, evidence.as_ref()).await;
    tracing::debug!(plan_id = %ir.plan_id, nodes = ir.nodes.len(), "plan created");

    // 5. Compile
    let graph = state.compiler.compile(ir).await?;
    tracing::debug!(
        graph_id = %graph.graph_id,
        estimated_cost = graph.metadata.estimated_cost,
        estimated_tokens = graph.metadata.estimated_tokens,
        "graph compiled"
    );

    // 6. Reserve resources
    state.resource_manager.reserve(&graph).await?;

    // 7. Schedule
    let reservation = ReservationId(Uuid::new_v4());
    let mut instance = state.scheduler.schedule(graph, reservation);

    // 8. Execute
    let result = state
        .scheduler
        .run(&mut instance, &*state.executor)
        .await
        .map_err(|e| anyhow::anyhow!("Execution failed: {}", e))?;

    tracing::debug!(
        instance_id = %result.instance_id,
        success = result.success,
        latency_ms = result.total_latency_ms,
        "execution complete"
    );

    // 9. Record telemetry
    for (node_id, state_val) in &instance.node_states {
        if let NodeState::Failed(_reason) = state_val {
            let record = ExecutionRecord {
                record_id: Uuid::new_v4(),
                plan_id: instance.instance_id,
                node_id: *node_id,
                model: request.model.clone(),
                provider: state.provider.name().to_string(),
                intent: reqs.intent_classification.clone(),
                latency_ms: result.total_latency_ms,
                tokens: 0,
                cost: 0.0,
                success: false,
                timestamp: chrono::Utc::now().timestamp(),
            };
            let _ = state.evidence_repository.record(record).await;
        }
    }

    // Record overall success
    let record = ExecutionRecord {
        record_id: Uuid::new_v4(),
        plan_id: instance.instance_id,
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

    // 10. Build response
    if result.success {
        Ok(ChatCompletionResponse {
            id: request_id.to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: request.model.clone(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: "Request processed successfully.".to_string(),
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Some(Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: result.total_tokens as u32,
            }),
        })
    } else {
        anyhow::bail!("Execution completed with failures")
    }
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
