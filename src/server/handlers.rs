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
use crate::context::assembler::DefaultContextAssembler;
use crate::executor::DefaultExecutor;
use crate::planner::Planner;
use crate::planner::SimplePlanner;
use crate::providers::ChatProvider;
use crate::requirements::extractor::RequirementsExtractor;
use crate::requirements::extractor::DefaultRequirementsExtractor;
use crate::resource::ResourceManager;
use crate::resource::DefaultResourceManager;
use crate::scheduler::Scheduler;
use crate::scheduler::default::DefaultScheduler;
use crate::strategies::consensus::ConsensusStrategy;
use crate::strategies::reflection::ReflectionStrategy;
use crate::strategies::single::SingleStrategy;
use crate::strategies::Strategy;
use crate::telemetry::EvidenceRepository;
use crate::types::*;

#[derive(Clone)]
pub struct AppState {
    pub context_assembler: Arc<DefaultContextAssembler>,
    pub requirements_extractor: Arc<DefaultRequirementsExtractor>,
    pub planner: Arc<SimplePlanner>,
    pub compiler: Arc<DefaultCompiler>,
    pub scheduler: Arc<DefaultScheduler>,
    pub executor: Arc<DefaultExecutor>,
    pub resource_manager: Arc<DefaultResourceManager>,
    pub evidence_repository: Arc<dyn EvidenceRepository + Send + Sync>,
    pub provider: Arc<dyn ChatProvider + Send + Sync>,
    pub config: Arc<AppConfig>,
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
        let planner = Arc::new(SimplePlanner);

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

        let executor = Arc::new(DefaultExecutor::new(
            provider.clone(),
            strategies,
        ));

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
    tracing::debug!(intent = ?reqs.intent, complexity = ?reqs.complexity, "requirements extracted");

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
                intent: reqs.intent.clone(),
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
        intent: reqs.intent,
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
