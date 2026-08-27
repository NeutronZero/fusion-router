use async_trait::async_trait;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::compiler::Compiler;
use crate::context::assembler::ContextAssembler;
use crate::executor::Executor;
use crate::planner::Planner;
use crate::requirements::extractor::RequirementsExtractor;
use crate::resource::{BudgetEnvelope, ResourceGuard, ResourceManager};
use crate::scheduler::Scheduler;
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
    pub execution_result: Option<ExecutionResult>,
    pub response: Option<ChatCompletionResponse>,
    pub budget_envelope: Option<BudgetEnvelope>,
    /// Operator-configured tool names a client may declare (review H2).
    /// `None` means unrestricted; an empty list permits no client tools.
    pub permitted_client_tools: Option<Vec<String>>,
    /// Dispatch capacity of the scheduler, used to size the per-request
    /// envelope's scheduling-round budget (review H4b). Defaults to 16.
    pub scheduler_round_capacity: Option<u64>,
}

impl PipelineContext {
    pub fn new(
        request_id: Uuid,
        request: ChatCompletionRequest,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            request_id,
            cancellation_token,
            request,
            assembled_context: None,
            requirements: None,
            evidence: None,
            ir: None,
            graph: None,
            execution_result: None,
            response: None,
            budget_envelope: None,
            permitted_client_tools: None,
            scheduler_round_capacity: None,
        }
    }
}

#[async_trait]
pub trait PipelineStep<Input, Output>: Send + Sync {
    async fn execute(&self, input: Input, ctx: &mut PipelineContext)
        -> Result<Output, RouterError>;
}

pub struct ContextAssemblyStep {
    pub assembler: Arc<dyn ContextAssembler + Send + Sync>,
}

#[async_trait]
impl PipelineStep<ChatCompletionRequest, ContextSnapshot> for ContextAssemblyStep {
    async fn execute(
        &self,
        request: ChatCompletionRequest,
        ctx: &mut PipelineContext,
    ) -> Result<ContextSnapshot, RouterError> {
        let snapshot =
            self.assembler
                .assemble(&request)
                .await
                .map_err(|e| RouterError::StageFailure {
                    stage: PipelineStage::ContextAssembly,
                    request_id: ctx.request_id,
                    message: e.to_string(),
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
    async fn execute(
        &self,
        context: ContextSnapshot,
        ctx: &mut PipelineContext,
    ) -> Result<Requirements, RouterError> {
        let mut reqs = self.extractor.extract(&context);
        reqs.execution_intent = ctx.request.execution.clone();
        reqs.output_preferences = ctx.request.output.clone();
        reqs.requested_strategy = ctx.request.strategy.clone();
        reqs.requested_model = Some(ctx.request.model.clone());
        ctx.requirements = Some(reqs.clone());
        Ok(reqs)
    }
}

pub struct EvidenceSnapshotStep {
    pub repository: Arc<dyn EvidenceRepository + Send + Sync>,
}

#[async_trait]
impl PipelineStep<(), EvidenceSnapshot> for EvidenceSnapshotStep {
    async fn execute(
        &self,
        _: (),
        ctx: &mut PipelineContext,
    ) -> Result<EvidenceSnapshot, RouterError> {
        let evidence = self
            .repository
            .snapshot()
            .await
            .map_err(|e| RouterError::StageFailure {
                stage: PipelineStage::EvidenceSnapshot,
                request_id: ctx.request_id,
                message: e.to_string(),
            })?;
        ctx.evidence = Some(evidence.clone());
        Ok(evidence)
    }
}

pub struct PlanningStep {
    pub planner: Arc<dyn Planner + Send + Sync>,
    pub policies: Vec<Policy>,
    pub policy_version: u64,
}

#[async_trait]
impl PipelineStep<(Requirements, Option<EvidenceSnapshot>), WorkflowIR> for PlanningStep {
    async fn execute(
        &self,
        (reqs, evidence): (Requirements, Option<EvidenceSnapshot>),
        ctx: &mut PipelineContext,
    ) -> Result<WorkflowIR, RouterError> {
        let ir = self
            .planner
            .plan_with_policy_version(
                &reqs,
                &self.policies,
                evidence.as_ref(),
                self.policy_version,
            )
            .await
            .map_err(|e| RouterError::CapacityExceeded {
                request_id: ctx.request_id,
                details: e.to_string(),
            })?;
        ctx.ir = Some(ir.clone());
        Ok(ir)
    }
}

pub struct CompilationStep {
    pub compiler: Arc<dyn Compiler + Send + Sync>,
}

#[async_trait]
impl PipelineStep<WorkflowIR, ExecutionGraph> for CompilationStep {
    async fn execute(
        &self,
        ir: WorkflowIR,
        ctx: &mut PipelineContext,
    ) -> Result<ExecutionGraph, RouterError> {
        let mut graph = self.compiler.compile(ir).await.map_err(|e| match &e {
            CompilerError::ValidationError { pass, message, .. } if pass == "policy" => {
                RouterError::PolicyDenied {
                    request_id: ctx.request_id,
                    rule_id: message
                        .split("Policy rule '")
                        .nth(1)
                        .and_then(|rest| rest.split('\'').next())
                        .unwrap_or("policy")
                        .to_string(),
                }
            }
            // Compile-time affordability check (estimate vs remaining quota):
            // a quota rejection is the caller's problem to retry later, not a
            // server fault — surface as 429 instead of 500.
            CompilerError::ValidationError { pass, .. } if pass == "budget_optimisation" => {
                RouterError::ResourceExhausted {
                    request_id: ctx.request_id,
                    details: e.to_string(),
                }
            }
            _ => RouterError::StageFailure {
                stage: PipelineStage::Compilation,
                request_id: ctx.request_id,
                message: e.to_string(),
            },
        })?;

        for node in &mut graph.nodes {
            if node.model.is_empty() {
                node.model = ctx.request.model.clone();
            }
            if matches!(
                node.kind,
                ExecutionNodeKind::LLMGenerate
                    | ExecutionNodeKind::LLMReview
                    | ExecutionNodeKind::LLMJudge
            ) {
                if let Some(ctx_snapshot) = &ctx.assembled_context {
                    let messages = serde_json::to_value(&ctx_snapshot.messages).map_err(|e| {
                        RouterError::StageFailure {
                            stage: PipelineStage::Compilation,
                            request_id: ctx.request_id,
                            message: format!("failed to serialize context messages: {e}"),
                        }
                    })?;
                    node.config.insert("messages".to_string(), messages.clone());
                    // Strategy sub-nodes (consensus members, judge, etc.) are
                    // prebuilt at compile time and never see the request
                    // context. Propagate the assembled messages into every LLM
                    // sub-node so their requests carry the user's input.
                    if let Some(subgraph) = node.subgraph.as_mut() {
                        for sub_node in &mut subgraph.nodes {
                            if matches!(
                                sub_node.kind,
                                ExecutionNodeKind::LLMGenerate
                                    | ExecutionNodeKind::LLMReview
                                    | ExecutionNodeKind::LLMJudge
                            ) {
                                sub_node
                                    .config
                                    .insert("messages".to_string(), messages.clone());
                            }
                        }
                    }
                }
                // Law 7 / ADR-037: the request-declared tools become the
                // per-request tool allowlist. The provider is only ever shown
                // the allowlisted definitions and nothing is auto-executed
                // unless `tools.allow_auto_exec` is enabled server-side.
                // Review H2: client declarations are additionally intersected
                // with the OPERATOR-configured permitted set so a request can
                // never reach a tool (e.g. shell_command) by simply naming it.
                let permitted: Option<&Vec<String>> =
                    ctx.permitted_client_tools.as_ref();
                if let Some(tools) = &ctx.request.tools {
                    if !tools.is_empty() {
                        let allowlist: Vec<serde_json::Value> = tools
                            .iter()
                            .filter(|t| match permitted {
                                Some(list) => list.contains(&t.name),
                                None => true,
                            })
                            .map(|t| serde_json::Value::String(t.name.clone()))
                            .collect();
                        let allowlist = serde_json::Value::Array(allowlist);
                        node.config
                            .insert("tool_allowlist".to_string(), allowlist.clone());
                        if let Some(subgraph) = node.subgraph.as_mut() {
                            for sub_node in &mut subgraph.nodes {
                                if matches!(
                                    sub_node.kind,
                                    ExecutionNodeKind::LLMGenerate
                                        | ExecutionNodeKind::LLMReview
                                        | ExecutionNodeKind::LLMJudge
                                ) {
                                    sub_node
                                        .config
                                        .insert("tool_allowlist".to_string(), allowlist.clone());
                                }
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
    async fn execute(
        &self,
        graph: ExecutionGraph,
        ctx: &mut PipelineContext,
    ) -> Result<ResourceGuard, RouterError> {
        if !self.resource_manager.try_reserve(&graph).await {
            return Err(RouterError::ResourceExhausted {
                request_id: ctx.request_id,
                details: "Daily resource quota exhausted".to_string(),
            });
        }

        // Initialize per-request budget envelope from global quota.
        // Review H4b: the envelope is bounded BOTH by a share of the daily
        // quota and by what this graph plausibly needs (estimated cost with
        // headroom), so a single request cannot reserve 20% of the daily
        // budget for a trivial workflow.
        let q = self.resource_manager.quota();
        let estimate_with_headroom = graph
            .metadata
            .estimated_cost
            .saturating_mul(3)
            .max(NanoUSD::from_nanos(10_000));
        let max_cost =
            NanoUSD::from_nanos(q.max_daily_cost.as_nanos() / 5).min(estimate_with_headroom);
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
            .filter(|n| {
                matches!(
                    n.kind,
                    ExecutionNodeKind::LLMGenerate
                        | ExecutionNodeKind::LLMReview
                        | ExecutionNodeKind::LLMJudge
                )
            })
            .count()
            .max(1) as u64;
        let max_output = ctx.request.max_tokens.unwrap_or(4096) as u64;
        let request_min = (input_tokens + max_output).saturating_mul(llm_node_count);
        let max_tokens = (q.max_daily_tokens / 5).max(request_min).max(10_000);
        // Review H4b: the iteration ceiling counts SCHEDULER ROUNDS (batched
        // dispatch waves), not loop-node iterations. Derive it from the graph
        // shape with generous loop slack instead of a hardcoded 100 that
        // aborted long graphs mid-run.
        let rounds_needed = (graph.nodes.len() as u64)
            .div_ceil(ctx.scheduler_round_capacity.unwrap_or(16))
            .max(1);
        let max_iterations = (rounds_needed.saturating_mul(4)).saturating_add(64) as u32;
        ctx.budget_envelope = Some(BudgetEnvelope::new(max_cost, max_tokens, max_iterations));

        let guard = ResourceGuard::new(ctx.request_id, graph, self.resource_manager.clone());
        Ok(guard)
    }
}

pub struct SchedulingExecutionStep {
    pub scheduler: Arc<dyn Scheduler + Send + Sync>,
    pub executor: Arc<dyn Executor + Send + Sync>,
}

#[async_trait]
impl PipelineStep<(ExecutionGraph, ReservationId), ExecutionResult> for SchedulingExecutionStep {
    async fn execute(
        &self,
        (graph, reservation): (ExecutionGraph, ReservationId),
        ctx: &mut PipelineContext,
    ) -> Result<ExecutionResult, RouterError> {
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
    async fn execute(
        &self,
        result: ExecutionResult,
        ctx: &mut PipelineContext,
    ) -> Result<ChatCompletionResponse, RouterError> {
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
                    result
                        .outputs
                        .get(&id)
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                })
            })
            .or_else(|| {
                result
                    .outputs
                    .values()
                    .last()
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
            })
            // Tool call results are structured JSON, not text: surface them so
            // the client can see what executed and what came back.
            .or_else(|| {
                result.outputs.values().last().and_then(|v| {
                    v.as_object()
                        .map(|o| serde_json::json!(o).to_string())
                        .or_else(|| v.as_array().map(|a| serde_json::json!(a).to_string()))
                })
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
                total_tokens: result.total_tokens.min(u32::MAX as u64) as u32,
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
            strategy: None,
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
        assert!(ctx.execution_result.is_none());
        assert!(ctx.response.is_none());
        assert!(ctx.budget_envelope.is_none());
    }

    #[test]
    fn test_pipeline_context_cancellation_token_carried() {
        let token = CancellationToken::new();
        let ctx = PipelineContext::new(Uuid::new_v4(), test_request(), token.clone());

        assert!(!ctx.cancellation_token.is_cancelled());
        token.cancel();
        assert!(ctx.cancellation_token.is_cancelled());
    }

    fn permissive_quota() -> crate::types::Quota {
        crate::types::Quota {
            max_daily_cost: crate::types::NanoUSD::from_nanos(1_000_000_000_000),
            max_daily_tokens: 1_000_000_000,
            max_concurrent: 100,
            provider_limits: std::collections::HashMap::new(),
        }
    }

    fn consensus_ir() -> WorkflowIR {
        WorkflowIR {
            plan_id: Uuid::new_v4(),
            nodes: vec![IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: crate::types::StrategyKind::Consensus,
                model: Some("test-model".into()),
                config: std::collections::HashMap::new(),
            }],
            edges: vec![],
            metadata: IRMetadata {
                policy_version: 0,
                policy_applied: vec![],
                estimated_cost: crate::types::NanoUSD::from_nanos(100_000_000),
                estimated_tokens: 500,
            },
        }
    }

    #[tokio::test]
    async fn test_compilation_step_propagates_tool_allowlist() {
        let compiler = Arc::new(crate::compiler::build_compiler(
            crate::types::ModelCatalog::default(),
            Arc::new(crate::resource::DefaultResourceManager::new(
                permissive_quota(),
            )),
            None,
        ));
        let mut request = test_request();
        request.tools = Some(vec![ToolDefinition {
            name: "calculator".into(),
            description: "arithmetic".into(),
            parameters: None,
        }]);

        let mut ctx = PipelineContext::new(Uuid::new_v4(), request, CancellationToken::new());
        let step = CompilationStep { compiler };
        let graph = step.execute(consensus_ir(), &mut ctx).await.unwrap();

        let node = &graph.nodes[0];
        let allowlist = node
            .config
            .get("tool_allowlist")
            .and_then(|v| v.as_array())
            .expect("top-level LLM node must carry the request tool allowlist");
        assert_eq!(allowlist.len(), 1);
        assert_eq!(allowlist[0], serde_json::json!("calculator"));

        let subgraph = node
            .subgraph
            .as_ref()
            .expect("consensus node must carry a compile-time subgraph");
        let sub_nodes_with_allowlist = subgraph
            .nodes
            .iter()
            .filter(|n| {
                n.config
                    .get("tool_allowlist")
                    .and_then(|v| v.as_array())
                    .is_some()
            })
            .count();
        assert!(
            sub_nodes_with_allowlist > 0,
            "LLM sub-nodes must receive the tool allowlist"
        );
    }

    #[tokio::test]
    async fn test_compilation_step_omits_allowlist_when_request_has_no_tools() {
        let compiler = Arc::new(crate::compiler::build_compiler(
            crate::types::ModelCatalog::default(),
            Arc::new(crate::resource::DefaultResourceManager::new(
                permissive_quota(),
            )),
            None,
        ));

        let mut ctx =
            PipelineContext::new(Uuid::new_v4(), test_request(), CancellationToken::new());
        let step = CompilationStep { compiler };
        let graph = step.execute(consensus_ir(), &mut ctx).await.unwrap();

        assert!(
            !graph.nodes[0].config.contains_key("tool_allowlist"),
            "no tools requested => no allowlist advertised (fail closed)"
        );
    }
}
