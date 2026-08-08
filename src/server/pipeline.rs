use std::sync::Arc;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::context::assembler::ContextAssembler;
use crate::planner::Planner;
use crate::compiler::Compiler;
use crate::scheduler::Scheduler;
use crate::executor::Executor;
use crate::requirements::extractor::RequirementsExtractor;
use crate::resource::{ResourceManager, ResourceGuard, BudgetEnvelope};
use crate::telemetry::EvidenceRepository;
use crate::types::*;

pub struct PipelineContext {
    pub request_id: Uuid,
    pub cancellation_token: CancellationToken,
    pub request: ChatCompletionRequest,
    pub assembled_context: Option<ContextSnapshot>,
    pub requirements: Option<Requirements>,
    pub evidence: Option<EvidenceSnapshot>,
    pub ir: Option<WorkflowIR>,
    pub graph: Option<ExecutionGraph>,
    pub resource_guard: Option<ResourceGuard>,
    pub execution_result: Option<ExecutionResult>,
    pub response: Option<ChatCompletionResponse>,
    pub budget_envelope: Option<BudgetEnvelope>,
}

impl PipelineContext {
    pub fn new(request_id: Uuid, request: ChatCompletionRequest, cancellation_token: CancellationToken) -> Self {
        Self {
            request_id,
            cancellation_token,
            request,
            assembled_context: None,
            requirements: None,
            evidence: None,
            ir: None,
            graph: None,
            resource_guard: None,
            execution_result: None,
            response: None,
            budget_envelope: None,
        }
    }
}

#[async_trait]
pub trait PipelineStep<Input, Output>: Send + Sync {
    async fn execute(&self, input: Input, ctx: &mut PipelineContext) -> Result<Output, RouterError>;
}

pub struct ContextAssemblyStep {
    pub assembler: Arc<dyn ContextAssembler + Send + Sync>,
}

#[async_trait]
impl PipelineStep<ChatCompletionRequest, ContextSnapshot> for ContextAssemblyStep {
    async fn execute(&self, request: ChatCompletionRequest, ctx: &mut PipelineContext) -> Result<ContextSnapshot, RouterError> {
        let snapshot = self.assembler.assemble(&request).await.map_err(|e| {
            RouterError::StageFailure {
                stage: PipelineStage::ContextAssembly,
                request_id: ctx.request_id,
                message: e.to_string(),
            }
        })?;
        ctx.assembled_context = Some(snapshot.clone());
        Ok(snapshot)
    }
}

pub struct RequirementsExtractionStep {
    pub extractor: Arc<dyn RequirementsExtractor + Send + Sync>,
}

#[async_trait]
impl PipelineStep<ContextSnapshot, Requirements> for RequirementsExtractionStep {
    async fn execute(&self, context: ContextSnapshot, ctx: &mut PipelineContext) -> Result<Requirements, RouterError> {
        let mut reqs = self.extractor.extract(&context);
        reqs.execution_intent = ctx.request.execution.clone();
        reqs.output_preferences = ctx.request.output.clone();
        ctx.requirements = Some(reqs.clone());
        Ok(reqs)
    }
}

pub struct EvidenceSnapshotStep {
    pub repository: Arc<dyn EvidenceRepository + Send + Sync>,
}

#[async_trait]
impl PipelineStep<(), Option<EvidenceSnapshot>> for EvidenceSnapshotStep {
    async fn execute(&self, _: (), ctx: &mut PipelineContext) -> Result<Option<EvidenceSnapshot>, RouterError> {
        let evidence = self.repository.snapshot().await.ok();
        ctx.evidence = evidence.clone();
        Ok(evidence)
    }
}

pub struct PlanningStep {
    pub planner: Arc<dyn Planner + Send + Sync>,
    pub policies: Vec<Policy>,
}

#[async_trait]
impl PipelineStep<(Requirements, Option<EvidenceSnapshot>), WorkflowIR> for PlanningStep {
    async fn execute(&self, (reqs, evidence): (Requirements, Option<EvidenceSnapshot>), ctx: &mut PipelineContext) -> Result<WorkflowIR, RouterError> {
        let ir = self.planner.plan(&reqs, &self.policies, evidence.as_ref()).await;
        ctx.ir = Some(ir.clone());
        Ok(ir)
    }
}

pub struct CompilationStep {
    pub compiler: Arc<dyn Compiler + Send + Sync>,
}

#[async_trait]
impl PipelineStep<WorkflowIR, ExecutionGraph> for CompilationStep {
    async fn execute(&self, ir: WorkflowIR, ctx: &mut PipelineContext) -> Result<ExecutionGraph, RouterError> {
        let mut graph = self.compiler.compile(ir).await.map_err(|e| {
            RouterError::StageFailure {
                stage: PipelineStage::Compilation,
                request_id: ctx.request_id,
                message: e.to_string(),
            }
        })?;

        for node in &mut graph.nodes {
            if node.model.is_empty() {
                node.model = ctx.request.model.clone();
            }
            if matches!(node.kind, ExecutionNodeKind::LLMGenerate | ExecutionNodeKind::LLMReview | ExecutionNodeKind::LLMJudge) {
                if let Some(ctx_snapshot) = &ctx.assembled_context {
                    let messages = serde_json::to_value(&ctx_snapshot.messages).unwrap_or_default();
                    node.config.insert("messages".to_string(), messages.clone());
                    // Strategy sub-nodes (consensus members, judge, etc.) are
                    // prebuilt at compile time and never see the request
                    // context. Propagate the assembled messages into every LLM
                    // sub-node so their requests carry the user's input.
                    if let Some(subgraph) = node.subgraph.as_mut() {
                        for sub_node in &mut subgraph.nodes {
                            if matches!(sub_node.kind, ExecutionNodeKind::LLMGenerate | ExecutionNodeKind::LLMReview | ExecutionNodeKind::LLMJudge) {
                                sub_node.config.insert("messages".to_string(), messages.clone());
                            }
                        }
                    }
                }
            }
        }

        ctx.graph = Some(graph.clone());
        Ok(graph)
    }
}

pub struct ResourceReservationStep {
    pub resource_manager: Arc<dyn ResourceManager>,
}

#[async_trait]
impl PipelineStep<ExecutionGraph, ResourceGuard> for ResourceReservationStep {
    async fn execute(&self, graph: ExecutionGraph, ctx: &mut PipelineContext) -> Result<ResourceGuard, RouterError> {
        if !self.resource_manager.try_reserve(&graph).await {
            return Err(RouterError::ResourceExhausted {
                request_id: ctx.request_id,
                details: "Daily resource quota exhausted".to_string(),
            });
        }

        // Initialize per-request budget envelope from global quota
        let q = self.resource_manager.quota();
        let max_cost = ((q.max_daily_cost * 0.2 * 1000.0) as u64).max(10_000);
        // The envelope must never be smaller than the request itself needs:
        // every LLM node re-sends the full assembled context, so the minimum
        // workable budget is (input + output) x number of LLM nodes.
        let input_tokens: u64 = ctx
            .assembled_context
            .as_ref()
            .map(|c| {
                c.messages
                    .iter()
                    .map(|m| (m.content.len() / 4) as u64)
                    .sum()
            })
            .unwrap_or(0);
        let llm_node_count = graph
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, ExecutionNodeKind::LLMGenerate | ExecutionNodeKind::LLMReview | ExecutionNodeKind::LLMJudge))
            .count()
            .max(1) as u64;
        let max_output = ctx.request.max_tokens.unwrap_or(4096) as u64;
        let request_min = (input_tokens + max_output).saturating_mul(llm_node_count);
        let max_tokens = (q.max_daily_tokens / 5).max(request_min).max(10_000);
        ctx.budget_envelope = Some(BudgetEnvelope::new(max_cost, max_tokens, 100));

        let guard = ResourceGuard::new(ctx.request_id, graph, self.resource_manager.clone());
        ctx.resource_guard = None;
        Ok(guard)
    }
}

pub struct SchedulingExecutionStep {
    pub scheduler: Arc<dyn Scheduler + Send + Sync>,
    pub executor: Arc<dyn Executor + Send + Sync>,
}

#[async_trait]
impl PipelineStep<(ExecutionGraph, ReservationId), ExecutionResult> for SchedulingExecutionStep {
    async fn execute(&self, (graph, reservation): (ExecutionGraph, ReservationId), ctx: &mut PipelineContext) -> Result<ExecutionResult, RouterError> {
        let mut instance = self.scheduler.schedule(graph, reservation);
        instance.budget_envelope = ctx.budget_envelope.clone();
        let result = self
            .scheduler
            .run_with_cancellation(&mut instance, &*self.executor, &ctx.cancellation_token)
            .await
            .map_err(|e| RouterError::StageFailure {
                stage: PipelineStage::Execution,
                request_id: ctx.request_id,
                message: e.to_string(),
            })?;

        ctx.execution_result = Some(result.clone());
        Ok(result)
    }
}

pub struct ResponseBuilderStep;

#[async_trait]
impl PipelineStep<ExecutionResult, ChatCompletionResponse> for ResponseBuilderStep {
    async fn execute(&self, result: ExecutionResult, ctx: &mut PipelineContext) -> Result<ChatCompletionResponse, RouterError> {
        if !result.success {
            return Err(RouterError::StageFailure {
                stage: PipelineStage::ResponseBuilding,
                request_id: ctx.request_id,
                message: "Execution completed with failures".to_string(),
            });
        }

        let content = result
            .final_output
            .as_ref()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .or_else(|| {
                result.terminal_node_id.and_then(|id| {
                    result.outputs.get(&id).and_then(|v| v.as_str().map(|s| s.to_string()))
                })
            })
            .or_else(|| {
                result.outputs.values().last().and_then(|v| v.as_str().map(|s| s.to_string()))
            })
            .unwrap_or_else(|| "Request processed successfully.".to_string());

        let response = ChatCompletionResponse {
            id: ctx.request_id.to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: ctx.request.model.clone(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content,
                },
                finish_reason: "stop".to_string(),
            }],
            native_tool_calls: None,
            usage: Some(Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: result.total_tokens as u32,
            }),
        };

        ctx.response = Some(response.clone());
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_request() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            files: None,
            execution: None,
            output: None,
        }
    }

    #[test]
    fn test_pipeline_context_new_initializes_state() {
        let request_id = Uuid::new_v4();
        let token = CancellationToken::new();
        let request = test_request();

        let ctx = PipelineContext::new(request_id, request, token.clone());

        assert_eq!(ctx.request_id, request_id);
        assert_eq!(ctx.request.model, "gpt-4o");
        assert!(ctx.assembled_context.is_none());
        assert!(ctx.requirements.is_none());
        assert!(ctx.evidence.is_none());
        assert!(ctx.ir.is_none());
        assert!(ctx.graph.is_none());
        assert!(ctx.resource_guard.is_none());
        assert!(ctx.execution_result.is_none());
        assert!(ctx.response.is_none());
        assert!(ctx.budget_envelope.is_none());
    }

    #[test]
    fn test_pipeline_context_cancellation_token_carried() {
        let token = CancellationToken::new();
        let ctx = PipelineContext::new(
            Uuid::new_v4(),
            test_request(),
            token.clone(),
        );

        assert!(!ctx.cancellation_token.is_cancelled());
        token.cancel();
        assert!(ctx.cancellation_token.is_cancelled());
    }
}
