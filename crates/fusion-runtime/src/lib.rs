//! Runtime engine — executes scheduled workflows against providers.
//!
//! Provides:
//! - `ChatProvider` trait for LLM dispatch
//! - `MockProvider` for deterministic testing
//! - `RuntimeEngine` that integrates scheduler + provider

use async_trait::async_trait;
use fusion_kernel::resource::ResourceManager;
use fusion_scheduler::{DefaultScheduler, ExecutionOutcome, Executor};
use fusion_types::{
    ExecutionGraph, ExecutionNode, ExecutionNodeKind, NodeExecContext, NodeExecutionResult,
    NodeState, StrategyKind, ToolCall, Usage,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Chat completion request sent to a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// A chat message with role and content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Response from a chat provider.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub usage: Option<Usage>,
    /// Provider-native tool calls. Executed by the runtime ONLY when they are
    /// allowlisted (Law 7 / ADR-037: fail closed by default).
    pub tool_calls: Vec<ToolCall>,
    /// Accumulated tool execution results from the agentic loop.
    pub tool_results: Vec<serde_json::Value>,
}

/// Trait for LLM chat providers. Implementors dispatch requests to actual
/// model endpoints. The runtime calls this for each LLM node in the graph.
#[async_trait]
pub trait ChatProvider: Send + Sync {
    async fn chat_completion(&self, request: &ChatRequest) -> Result<ChatResponse, String>;
}

/// Mock provider that returns fixed responses. Used for deterministic testing.
pub struct MockProvider {
    response_prefix: String,
}

impl MockProvider {
    pub fn new(response_prefix: impl Into<String>) -> Self {
        Self {
            response_prefix: response_prefix.into(),
        }
    }

    pub fn default_response() -> Self {
        Self::new("mock response")
    }
}

#[async_trait]
impl ChatProvider for MockProvider {
    async fn chat_completion(&self, request: &ChatRequest) -> Result<ChatResponse, String> {
        Ok(ChatResponse {
            content: format!("{} for model {}", self.response_prefix, request.model),
            usage: Some(Usage {
                prompt_tokens: 50,
                completion_tokens: 25,
                total_tokens: 75,
            }),
            tool_calls: vec![],
            tool_results: Vec::new(),
        })
    }
}

/// Spy provider that records the last request for assertions.
pub struct SpyProvider {
    pub last_request: std::sync::Mutex<Option<ChatRequest>>,
    response_prefix: String,
}

impl Default for SpyProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SpyProvider {
    pub fn new() -> Self {
        Self {
            last_request: std::sync::Mutex::new(None),
            response_prefix: "spy response".into(),
        }
    }

    pub fn last_request(&self) -> Option<ChatRequest> {
        self.last_request.lock().unwrap().clone()
    }
}

#[async_trait]
impl ChatProvider for SpyProvider {
    async fn chat_completion(&self, request: &ChatRequest) -> Result<ChatResponse, String> {
        *self.last_request.lock().unwrap() = Some(request.clone());
        Ok(ChatResponse {
            content: format!("{} for model {}", self.response_prefix, request.model),
            usage: Some(Usage {
                prompt_tokens: 50,
                completion_tokens: 25,
                total_tokens: 75,
            }),
            tool_calls: vec![],
            tool_results: Vec::new(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tools (Phase 4.5) — fail-closed by default
// ---------------------------------------------------------------------------

/// A tool executable by the runtime. Tools are only invoked when the node's
/// `tool_allowlist` config names them AND auto-execution is enabled.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn execute(&self, arguments: serde_json::Value) -> Result<serde_json::Value, String>;
}

/// Authority consulted before a policy-inserted `Gate` node may pass
/// (review H1). Gates previously succeeded instantly, making the policy
/// `Approval` effect a no-op. With no store configured the gate FAILS
/// CLOSED: execution cannot proceed past an unapproved gate.
#[async_trait]
pub trait ApprovalGate: Send + Sync {
    async fn is_approved(&self, gate_id: uuid::Uuid) -> bool;
}

/// Simple shared-memory approval store. Hosts can pre-register approvals
/// (e.g. from an operations API), keyed by the deterministic gate id that
/// `PolicyCompilerPass` derives via
/// `Uuid::new_v5(target_node_id, b"policy_approval_gate")`.
#[derive(Default, Clone)]
pub struct InMemoryApprovalStore {
    approved: std::collections::HashSet<uuid::Uuid>,
}

impl InMemoryApprovalStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn approve(&mut self, gate_id: uuid::Uuid) {
        self.approved.insert(gate_id);
    }

    /// Convenience for hosts approving the gate a policy pass created for
    /// `target_node_id`, without re-deriving the namespace constant.
    pub fn approve_target(&mut self, target_node_id: uuid::Uuid) {
        self.approved
            .insert(uuid::Uuid::new_v5(&target_node_id, b"policy_approval_gate"));
    }
}

#[async_trait]
impl ApprovalGate for InMemoryApprovalStore {
    async fn is_approved(&self, gate_id: uuid::Uuid) -> bool {
        self.approved.contains(&gate_id)
    }
}

/// In-memory tool registry.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: std::collections::HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}

/// Provider-backed executor that satisfies the scheduler's `Executor` trait.
/// Routes each node's model string to the chat provider.
pub struct ProviderExecutor {
    provider: Arc<dyn ChatProvider>,
    tools: ToolRegistry,
    allow_auto_exec: bool,
    /// Optional budget envelope (Phase 4.6). When set, each node's provider
    /// calls are gated by `can_afford` and actual usage is recorded.
    resource_manager: Option<Arc<dyn ResourceManager>>,
    /// Model-aware pricing used to record REAL cost against the resource
    /// manager (review H3b: previously recorded `NanoUSD::ZERO`, so daily
    /// cost quotas never accrued on this path).
    pricing: Option<fusion_scheduler::PricingResolver>,
    /// Approval authority for policy-inserted `Gate` nodes (review H1).
    /// `None` means fail closed: every gate blocks.
    approvals: Option<std::sync::Arc<dyn ApprovalGate>>,
}

impl ProviderExecutor {
    pub fn new(provider: Arc<dyn ChatProvider>) -> Self {
        Self {
            provider,
            tools: ToolRegistry::new(),
            allow_auto_exec: false,
            resource_manager: None,
            pricing: None,
            approvals: None,
        }
    }

    pub fn with_tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_allow_auto_exec(mut self, allow: bool) -> Self {
        self.allow_auto_exec = allow;
        self
    }

    pub fn with_resource_manager(mut self, rm: Arc<dyn ResourceManager>) -> Self {
        self.resource_manager = Some(rm);
        self
    }

    /// Installs model-aware pricing so recorded usage carries real cost.
    pub fn with_pricing(
        mut self,
        resolver: fusion_scheduler::PricingResolver,
    ) -> Self {
        self.pricing = Some(resolver);
        self
    }

    /// Installs an approval authority for policy `Gate` nodes. Without one,
    /// gates fail closed (review H1).
    pub fn with_approvals(mut self, approvals: std::sync::Arc<dyn ApprovalGate>) -> Self {
        self.approvals = Some(approvals);
        self
    }

    fn price_for(&self, model: &str) -> fusion_scheduler::TokenPricing {
        match &self.pricing {
            Some(resolve) => resolve(model),
            None => fusion_scheduler::TokenPricing::flat_fallback(),
        }
    }

    /// Builds a `ChatRequest` from node config and execution context.
    ///
    /// Order:
    /// 1. `config["messages"]` (JSON array of {role, content}) if present
    /// 2. System prompt injected by kind (Judge) / strategy (Reflection) when
    ///    no system message exists
    /// 3. Parent outputs appended as user messages (Judge / Review / Generate)
    /// Errors when `config["messages"]` is present but malformed: silently
    /// executing with an empty prompt previously swallowed operator mistakes
    /// and sent context-free requests upstream (review L4).
    pub fn build_request(
        &self,
        node: &ExecutionNode,
        ctx: &NodeExecContext,
    ) -> Result<ChatRequest, String> {
        let mut messages: Vec<ChatMessage> = match node.config.get("messages") {
            Some(v) => serde_json::from_value(v.clone())
                .map_err(|e| format!("node {}: invalid 'messages' config: {e}", node.id))?,
            None => Vec::new(),
        };

        let temperature = node
            .config
            .get("temperature")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32);

        let max_tokens = node
            .config
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        if !messages.iter().any(|m| m.role == "system") {
            let system_prompt = match node.kind {
                ExecutionNodeKind::LLMJudge => Some(
                    "You are a judge evaluating the quality and correctness of responses. \
                     Assess the provided answers critically and select the best one, explaining your reasoning.",
                ),
                _ => match node.strategy {
                    StrategyKind::Reflection => Some(
                        "You are a reflective reviewer. Analyze the previous response, identify \
                         potential issues, and provide an improved version.",
                    ),
                    _ => None,
                },
            };
            if let Some(prompt) = system_prompt {
                messages.insert(
                    0,
                    ChatMessage {
                        role: "system".into(),
                        content: prompt.to_string(),
                    },
                );
            }
        }

        // Append parent outputs as user context (P2 indirect prompt injection:
        // wrap untrusted content with delimiters so judge/reviewer prompts can
        // instruct the model to treat it as data, not instruction).
        if !ctx.parent_outputs.is_empty() {
            match node.kind {
                ExecutionNodeKind::LLMJudge
                | ExecutionNodeKind::LLMReview
                | ExecutionNodeKind::LLMGenerate => {
                    for (parent_id, output) in &ctx.parent_outputs {
                        messages.push(ChatMessage {
                            role: "user".into(),
                            content: format!(
                                "Context from parent node {}:\n<<BEGIN_UNTRUSTED_CONTEXT>>\n{}\n<<END_UNTRUSTED_CONTEXT>>",
                                parent_id, output
                            ),
                        });
                    }
                }
                _ => {}
            }
        }

        Ok(ChatRequest {
            model: node.model.clone(),
            messages,
            temperature,
            max_tokens,
        })
    }

    /// Executes a prebuilt `ExecutionSubgraph` in topological order.
    ///
    /// Parent outputs from the outer context are shared into sub-node contexts.
    /// Each sub-node executes via `execute_node` recursively, so nested
    /// subgraphs and control-flow kinds work. The exit node's output is
    /// returned as the parent node's output.
    async fn execute_subgraph(
        &self,
        node: &ExecutionNode,
        subgraph: &fusion_types::ExecutionSubgraph,
        ctx: &NodeExecContext,
    ) -> NodeExecutionResult {
        use std::collections::{HashMap as StdHashMap, HashSet};

        let mut outputs: StdHashMap<uuid::Uuid, serde_json::Value> = StdHashMap::new();
        let mut completed: HashSet<uuid::Uuid> = HashSet::new();
        // Review H3c: track prompt/completion separately so the aggregate
        // usage is truthful (previously everything landed in prompt_tokens,
        // completion was hardcoded 0, and a computed cost was discarded).
        let mut total_prompt_tokens: u64 = 0;
        let mut total_completion_tokens: u64 = 0;
        let start = std::time::Instant::now();

        // Topological execution loop
        while completed.len() < subgraph.nodes.len() {
            // Find ready sub-nodes (all incoming edges satisfied)
            let ready: Vec<&ExecutionNode> = subgraph
                .nodes
                .iter()
                .filter(|n| !completed.contains(&n.id))
                .filter(|n| {
                    subgraph
                        .edges
                        .iter()
                        .filter(|e| e.to == n.id)
                        .all(|e| completed.contains(&e.from))
                })
                .collect();

            if ready.is_empty() {
                return NodeExecutionResult {
                    state: NodeState::Failed("subgraph cycle or dangling edge".into()),
                    usage: None,
                    latency_ms: start.elapsed().as_millis() as u64,
                    output: None,
                };
            }

            for sub_node in ready {
                // Build sub-node context: outer parents + inner sub-node outputs
                let mut parent_outputs = ctx.parent_outputs.clone();
                for edge in &subgraph.edges {
                    if edge.to == sub_node.id {
                        if let Some(out) = outputs.get(&edge.from) {
                            parent_outputs.insert(edge.from, out.clone());
                        }
                    }
                }
                let sub_ctx = NodeExecContext {
                    parent_outputs,
                    graph_outputs: outputs.clone(),
                };
                let result = self.execute_node(sub_node, &sub_ctx).await;
                match result.state {
                    NodeState::Succeeded => {
                        completed.insert(sub_node.id);
                        if let Some(out) = result.output {
                            outputs.insert(sub_node.id, out);
                        }
                        if let Some(ref usage) = result.usage {
                            total_prompt_tokens += usage.prompt_tokens as u64;
                            total_completion_tokens += usage.completion_tokens as u64;
                        }
                    }
                    NodeState::Failed(msg) => {
                        return NodeExecutionResult {
                            state: NodeState::Failed(format!(
                                "subgraph node {} failed: {msg}",
                                sub_node.id
                            )),
                            usage: None,
                            latency_ms: start.elapsed().as_millis() as u64,
                            output: None,
                        };
                    }
                    _ => {
                        completed.insert(sub_node.id);
                    }
                }
            }
        }

        let exit_output = outputs
            .get(&subgraph.exit_node_id)
            .cloned()
            .unwrap_or_else(|| serde_json::json!({"subgraph": "complete"}));

        let total_tokens = total_prompt_tokens.saturating_add(total_completion_tokens);
        NodeExecutionResult {
            state: NodeState::Succeeded,
            usage: Some(Usage {
                prompt_tokens: total_prompt_tokens.min(u32::MAX as u64) as u32,
                completion_tokens: total_completion_tokens.min(u32::MAX as u64) as u32,
                total_tokens: total_tokens.min(u32::MAX as u64) as u32,
            }),
            latency_ms: start.elapsed().as_millis() as u64,
            output: Some(serde_json::json!({
                "subgraph": true,
                "exit_node_id": subgraph.exit_node_id.to_string(),
                "exit_output": exit_output,
                "node_id": node.id.to_string(),
            })),
        }
    }

    /// Single chat round-trip with optional tool execution loop.
    ///
    /// If the provider returns tool calls, each call is executed ONLY when
    /// auto-execution is enabled AND the tool is named in the node's
    /// `tool_allowlist` config AND present in the registry (fail closed).
    /// Tool results are appended as a `tool` message and the loop repeats
    /// until the provider returns plain content or `max_tool_iterations` is
    /// reached.
    ///
    /// When a `resource_manager` is attached, every provider call is gated by
    /// `can_afford` and actual usage is recorded on success (Phase 4.6).
    async fn roundtrip(
        &self,
        node: &ExecutionNode,
        request: ChatRequest,
    ) -> Result<ChatResponse, String> {
        let max_iterations = node
            .config
            .get("max_tool_iterations")
            .and_then(|v| v.as_u64())
            .unwrap_or(8)
            .clamp(1, 32) as usize;
        let mut request = request;
        let mut all_tool_results: Vec<serde_json::Value> = Vec::new();

        for iteration in 0..=max_iterations {
            if let Some(rm) = &self.resource_manager {
                let estimated_cost = node
                    .config
                    .get("budget_estimated_cost")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.001);
                let estimated_tokens = node
                    .config
                    .get("budget_estimated_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if !rm
                    .can_afford(
                        fusion_core::NanoUSD::checked_from_decimal_usd(&format!(
                            "{:.9}",
                            estimated_cost
                        ))
                        .unwrap_or(fusion_core::NanoUSD::ZERO),
                        estimated_tokens,
                    )
                    .await
                {
                    return Err(format!(
                        "budget exceeded for node {}: cannot afford estimated \
                         cost {estimated_cost} / tokens {estimated_tokens}",
                        node.id
                    ));
                }
            }

            let response = self.provider.chat_completion(&request).await?;
            if let Some(rm) = &self.resource_manager {
                if let Some(usage) = &response.usage {
                    // Record REAL cost (review H3b): pricing is model-aware
                    // when installed, conservative flat otherwise. Zero-cost
                    // recording previously meant daily cost quotas never
                    // accrued from this path.
                    let price = self.price_for(&request.model);
                    let cost = fusion_core::NanoUSD::from_nanos(
                        (usage.prompt_tokens as u64)
                            .saturating_mul(price.input_nanos_per_token)
                            .saturating_add(
                                (usage.completion_tokens as u64)
                                    .saturating_mul(price.output_nanos_per_token),
                            ),
                    );
                    rm.record_usage(cost, usage.total_tokens as u64).await;
                }
            }
            if response.tool_calls.is_empty() {
                return Ok(ChatResponse {
                    content: response.content,
                    usage: response.usage,
                    tool_calls: response.tool_calls,
                    tool_results: all_tool_results,
                });
            }

            if iteration == max_iterations {
                return Err(format!(
                    "exceeded max_tool_iterations ({max_iterations}) for node {}",
                    node.id
                ));
            }
            if !self.allow_auto_exec {
                return Err(format!(
                    "provider requested tool call(s) but auto-exec is disabled (node {})",
                    node.id
                ));
            }

            let allowlist: Vec<String> = node
                .config
                .get("tool_allowlist")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            let mut results: Vec<serde_json::Value> = Vec::new();
            for call in &response.tool_calls {
                if !allowlist.contains(&call.name) {
                    let entry = serde_json::json!({
                        "name": call.name,
                        "arguments": call.arguments,
                        "executed": false,
                        "error": true,
                        "reason": format!("tool '{}' not allowed for node {} (fail closed: allowlist)", call.name, node.id),
                    });
                    all_tool_results.push(entry.clone());
                    results.push(entry);
                    continue;
                }
                let tool = match self.tools.get(&call.name) {
                    Some(t) => t,
                    None => {
                        let entry = serde_json::json!({
                            "name": call.name,
                            "arguments": call.arguments,
                            "executed": false,
                            "error": true,
                            "reason": format!("tool '{}' not registered (node {})", call.name, node.id),
                        });
                        all_tool_results.push(entry.clone());
                        results.push(entry);
                        continue;
                    }
                };
                let outcome = tool.execute(call.arguments.clone()).await;
                let entry = serde_json::json!({
                    "name": call.name,
                    "arguments": call.arguments,
                    "executed": outcome.is_ok(),
                    "error": outcome.is_err(),
                    "result": outcome.unwrap_or_else(|e| serde_json::json!({ "error": e })),
                });
                all_tool_results.push(entry.clone());
                results.push(entry);
            }

            request.messages.push(ChatMessage {
                role: "tool".into(),
                content: serde_json::to_string(&results)
                    .map_err(|e| format!("failed to encode tool results: {e}"))?,
            });
        }

        Err(format!(
            "tool loop terminated unexpectedly for node {}",
            node.id
        ))
    }
}

#[async_trait]
impl Executor for ProviderExecutor {
    async fn execute_node(
        &self,
        node: &ExecutionNode,
        ctx: &NodeExecContext,
    ) -> NodeExecutionResult {
        // Prebuilt subgraph (Phase 4.3): execute inner nodes in dependency
        // order, propagating outputs; the exit node's output is the result.
        if let Some(subgraph) = &node.subgraph {
            return self.execute_subgraph(node, subgraph, ctx).await;
        }

        if matches!(node.kind, ExecutionNodeKind::Gate) {
            // Policy approval gate: FAIL CLOSED unless an approval authority
            // explicitly approves this gate/target pair (review H1). Gates
            // previously succeeded instantly, making the policy `Approval`
            // effect a silent no-op.
            let approved = match &self.approvals {
                Some(store) => store.is_approved(node.id).await,
                None => false,
            };
            return if approved {
                NodeExecutionResult {
                    state: NodeState::Succeeded,
                    usage: None,
                    latency_ms: 0,
                    output: Some(serde_json::json!({
                        "kind": "Gate",
                        "node_id": node.id.to_string(),
                        "approved": true,
                    })),
                }
            } else {
                NodeExecutionResult {
                    state: NodeState::Failed(
                        "policy approval required: gate not approved (fail closed)".into(),
                    ),
                    usage: None,
                    latency_ms: 0,
                    output: None,
                }
            };
        }

        if matches!(
            node.kind,
            ExecutionNodeKind::Transform
                | ExecutionNodeKind::Conditional
                | ExecutionNodeKind::Loop
                | ExecutionNodeKind::Split
                | ExecutionNodeKind::Join
                | ExecutionNodeKind::Barrier
        ) {
            // Control-flow / non-LLM nodes: no provider call.
            return NodeExecutionResult {
                state: NodeState::Succeeded,
                usage: None,
                latency_ms: 0,
                output: Some(serde_json::json!({
                    "kind": format!("{:?}", node.kind),
                    "node_id": node.id.to_string(),
                })),
            };
        }

        let start = std::time::Instant::now();
        let mut last_error: Option<String> = None;

        // Primary model attempts: 1 + max_retries (saturating so u32::MAX
        // cannot overflow).
        let mut attempts = node.retry_policy.max_retries.saturating_add(1);
        loop {
            if attempts == 0 {
                break;
            }
            attempts -= 1;

            let request = match self.build_request(node, ctx) {
                Ok(r) => r,
                Err(e) => {
                    return NodeExecutionResult {
                        state: NodeState::Failed(e),
                        usage: None,
                        latency_ms: start.elapsed().as_millis() as u64,
                        output: None,
                    };
                }
            };
            match self.roundtrip(node, request).await {
                Ok(response) => {
                    let mut output = serde_json::json!({
                        "content": response.content,
                        "node_id": node.id.to_string(),
                    });
                    if !response.tool_results.is_empty() {
                        output["tool_calls"] = serde_json::json!(response.tool_results);
                    }
                    return NodeExecutionResult {
                        state: NodeState::Succeeded,
                        usage: response.usage,
                        latency_ms: start.elapsed().as_millis() as u64,
                        output: Some(output),
                    };
                }
                Err(e) => {
                    last_error = Some(e.clone());
                    if attempts > 0 && node.retry_policy.backoff_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            node.retry_policy.backoff_ms,
                        ))
                        .await;
                    }
                }
            }
        }

        // Fallback model attempts (1 try)
        if let Some(fallback) = &node.fallback {
            let mut fallback_request = match self.build_request(node, ctx) {
                Ok(r) => r,
                Err(e) => {
                    return NodeExecutionResult {
                        state: NodeState::Failed(e),
                        usage: None,
                        latency_ms: start.elapsed().as_millis() as u64,
                        output: None,
                    };
                }
            };
            fallback_request.model = fallback.model.clone();
            match self.roundtrip(node, fallback_request).await {
                Ok(response) => {
                    return NodeExecutionResult {
                        state: NodeState::Succeeded,
                        usage: response.usage,
                        latency_ms: start.elapsed().as_millis() as u64,
                        output: Some(serde_json::json!({
                            "content": response.content,
                            "node_id": node.id.to_string(),
                            "fallback_model": fallback.model,
                        })),
                    };
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }

        NodeExecutionResult {
            state: NodeState::Failed(last_error.unwrap_or_else(|| "provider error".into())),
            usage: None,
            latency_ms: start.elapsed().as_millis() as u64,
            output: None,
        }
    }
}

/// Full runtime engine that integrates the scheduler with a chat provider.
pub struct RuntimeEngine {
    scheduler: DefaultScheduler,
    provider: Arc<dyn ChatProvider>,
    tools: ToolRegistry,
    allow_auto_exec: bool,
    resource_manager: Option<Arc<dyn ResourceManager>>,
    pricing: Option<fusion_scheduler::PricingResolver>,
    approvals: Option<Arc<dyn ApprovalGate>>,
}

impl RuntimeEngine {
    pub fn new(provider: Arc<dyn ChatProvider>) -> Self {
        Self {
            scheduler: DefaultScheduler::new(),
            provider,
            tools: ToolRegistry::new(),
            allow_auto_exec: false,
            resource_manager: None,
            pricing: None,
            approvals: None,
        }
    }

    pub fn with_max_concurrent(provider: Arc<dyn ChatProvider>, max_concurrent: usize) -> Self {
        Self {
            scheduler: DefaultScheduler::with_max_concurrent(max_concurrent),
            provider,
            tools: ToolRegistry::new(),
            allow_auto_exec: false,
            resource_manager: None,
            pricing: None,
            approvals: None,
        }
    }

    /// Registers a tool for potential execution by LLM nodes.
    pub fn with_tool(mut self, tool: Arc<dyn Tool>) -> Self {
        self.tools.register(tool);
        self
    }

    /// Enables auto-execution of allowlisted tool calls (default: false).
    pub fn with_allow_auto_exec(mut self, allow: bool) -> Self {
        self.allow_auto_exec = allow;
        self
    }

    /// Attaches an optional budget envelope (Phase 4.6). Each node's provider
    /// calls are gated by `can_afford`; actual usage is recorded on success.
    pub fn with_resource_manager(mut self, rm: Arc<dyn ResourceManager>) -> Self {
        self.resource_manager = Some(rm);
        self
    }

    /// Installs model-aware pricing so recorded usage carries real cost.
    pub fn with_pricing(mut self, resolver: fusion_scheduler::PricingResolver) -> Self {
        self.pricing = Some(resolver);
        self
    }

    /// Installs an approval authority for policy `Gate` nodes (fail closed
    /// without one).
    pub fn with_approvals(mut self, approvals: Arc<dyn ApprovalGate>) -> Self {
        self.approvals = Some(approvals);
        self
    }

    /// Execute a full execution graph to completion.
    pub async fn run(&self, graph: Arc<ExecutionGraph>) -> Result<ExecutionOutcome, String> {
        let executor = ProviderExecutor {
            provider: self.provider.clone(),
            tools: self.tools.clone(),
            allow_auto_exec: self.allow_auto_exec,
            resource_manager: self.resource_manager.clone(),
            pricing: self.pricing.clone(),
            approvals: self.approvals.clone(),
        };
        self.scheduler
            .run(graph, &executor)
            .await
            .map_err(|e| format!("{:?}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_core::NanoUSD;
    use fusion_types::*;
    use std::collections::HashMap;

    fn make_simple_graph() -> Arc<ExecutionGraph> {
        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();
        Arc::new(ExecutionGraph {
            graph_id: uuid::Uuid::new_v4(),
            nodes: vec![
                ExecutionNode {
                    id: n1,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "test-model".into(),
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
                    fallback: None,
                    config: std::collections::HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: n2,
                    kind: ExecutionNodeKind::LLMReview,
                    strategy: StrategyKind::Single,
                    model: "review-model".into(),
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
                    fallback: None,
                    config: std::collections::HashMap::new(),
                    subgraph: None,
                },
            ],
            edges: vec![ExecutionEdge {
                from: n1,
                to: n2,
                condition: None,
            }],
            metadata: GraphMetadata {
                policy_version: 0,
                estimated_cost: NanoUSD::from_nanos(10_000_000),
                estimated_tokens: 200,
                max_depth: 2,
                node_count: 2,
            },
            total_tokens: 200,
            total_cost: NanoUSD::from_nanos(10),
            primitive_graph_hash: 0,
        })
    }

    #[tokio::test]
    async fn test_mock_provider_returns_fixed_response() {
        let provider = MockProvider::new("hello");
        let response = provider
            .chat_completion(&ChatRequest {
                model: "gpt-4".into(),
                messages: vec![],
                temperature: None,
                max_tokens: None,
            })
            .await
            .expect("chat");
        assert!(response.content.contains("hello"));
        assert!(response.usage.is_some());
    }

    #[tokio::test]
    async fn test_runtime_executes_simple_graph() {
        let provider: Arc<dyn ChatProvider> = Arc::new(MockProvider::default_response());
        let engine = RuntimeEngine::new(provider);
        let graph = make_simple_graph();
        let outcome = engine.run(graph).await.expect("run");
        assert!(outcome.success);
        assert_eq!(outcome.outputs.len(), 2);
    }

    #[tokio::test]
    async fn test_runtime_records_usage() {
        let provider: Arc<dyn ChatProvider> = Arc::new(MockProvider::default_response());
        let engine = RuntimeEngine::new(provider);
        let graph = make_simple_graph();
        let outcome = engine.run(graph).await.expect("run");
        assert!(outcome.total_tokens > 0);
    }

    #[test]
    fn test_build_request_messages_from_config() {
        let executor = ProviderExecutor::new(Arc::new(MockProvider::default_response()));
        let node = ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: "m".into(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config: HashMap::from([
                (
                    "messages".into(),
                    serde_json::json!([
                        {"role": "user", "content": "hello from config"}
                    ]),
                ),
                ("temperature".into(), serde_json::json!(0.7)),
                ("max_tokens".into(), serde_json::json!(512)),
            ]),
            subgraph: None,
        };
        let request = executor.build_request(&node, &NodeExecContext::default()).unwrap();
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].role, "user");
        assert_eq!(request.messages[0].content, "hello from config");
        assert_eq!(request.temperature, Some(0.7));
        assert_eq!(request.max_tokens, Some(512));
    }

    #[test]
    fn test_build_request_judge_gets_system_prompt_and_parent_context() {
        let executor = ProviderExecutor::new(Arc::new(MockProvider::default_response()));
        let parent_id = uuid::Uuid::new_v4();
        let node = ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMJudge,
            strategy: StrategyKind::Single,
            model: "judge".into(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config: HashMap::new(),
            subgraph: None,
        };
        let ctx = NodeExecContext {
            parent_outputs: HashMap::from([(parent_id, serde_json::json!({"answer": "42"}))]),
            graph_outputs: HashMap::new(),
        };
        let request = executor.build_request(&node, &ctx).unwrap();
        // system + 1 parent context message
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, "system");
        assert!(request.messages[0].content.contains("judge"));
        assert_eq!(request.messages[1].role, "user");
        assert!(request.messages[1].content.contains(&parent_id.to_string()));
        assert!(request.messages[1].content.contains("42"));
    }

    #[test]
    fn test_build_request_system_prompt_inserted_once() {
        let executor = ProviderExecutor::new(Arc::new(MockProvider::default_response()));
        // Config already has a system message: no injection
        let node = ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMJudge,
            strategy: StrategyKind::Single,
            model: "judge".into(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config: HashMap::from([(
                "messages".into(),
                serde_json::json!([
                    {"role": "system", "content": "custom system"}
                ]),
            )]),
            subgraph: None,
        };
        let request = executor.build_request(&node, &NodeExecContext::default()).unwrap();
        assert_eq!(
            request
                .messages
                .iter()
                .filter(|m| m.role == "system")
                .count(),
            1,
            "system prompt must be inserted exactly once"
        );
        assert_eq!(request.messages[0].content, "custom system");
    }

    // -----------------------------------------------------------------------
    // Phase 4.2: retry + fallback tests
    // -----------------------------------------------------------------------

    /// Fails the first `fails_before_success` calls, then succeeds.
    pub struct FlakyProvider {
        fails_before_success: u32,
        attempts: std::sync::atomic::AtomicU32,
    }

    impl FlakyProvider {
        pub fn new(fails_before_success: u32) -> Self {
            Self {
                fails_before_success,
                attempts: std::sync::atomic::AtomicU32::new(0),
            }
        }

        pub fn attempt_count(&self) -> u32 {
            self.attempts.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl ChatProvider for FlakyProvider {
        async fn chat_completion(&self, request: &ChatRequest) -> Result<ChatResponse, String> {
            let attempt = self
                .attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if attempt < self.fails_before_success {
                Err(format!("boom attempt {attempt}"))
            } else {
                Ok(ChatResponse {
                    content: format!("recovered for model {}", request.model),
                    usage: Some(Usage {
                        prompt_tokens: 10,
                        completion_tokens: 5,
                        total_tokens: 15,
                    }),
                    tool_calls: vec![],
                    tool_results: Vec::new(),
                })
            }
        }
    }

    /// Always fails, recording whether the fallback model was tried.
    pub struct FailingProvider {
        fallback_attempts: std::sync::Mutex<Vec<String>>,
    }

    impl FailingProvider {
        pub fn new() -> Self {
            Self {
                fallback_attempts: std::sync::Mutex::new(Vec::new()),
            }
        }

        pub fn fallback_attempts(&self) -> Vec<String> {
            self.fallback_attempts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ChatProvider for FailingProvider {
        async fn chat_completion(&self, request: &ChatRequest) -> Result<ChatResponse, String> {
            self.fallback_attempts
                .lock()
                .unwrap()
                .push(request.model.clone());
            Err("always fails".into())
        }
    }

    fn make_llm_node(model: &str, retries: u32, fallback: Option<&str>) -> ExecutionNode {
        ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: model.into(),
            retry_policy: RetryPolicy {
                max_retries: retries,
                backoff_ms: 0,
            },
            fallback: fallback.map(|m| FallbackConfig {
                model: m.into(),
                provider: "fallback".into(),
            }),
            config: HashMap::new(),
            subgraph: None,
        }
    }

    #[tokio::test]
    async fn test_retry_succeeds_on_second_attempt() {
        let provider = Arc::new(FlakyProvider::new(1));
        let executor = ProviderExecutor::new(provider.clone());
        let node = make_llm_node("primary-model", 2, None);
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert!(
            matches!(result.state, NodeState::Succeeded),
            "retry should succeed"
        );
        assert_eq!(
            provider.attempt_count(),
            2,
            "2 attempts: 1 fail + 1 success"
        );
    }

    #[tokio::test]
    async fn test_max_retries_u32_max_does_not_overflow_attempts() {
        let provider = Arc::new(FlakyProvider::new(0));
        let executor = ProviderExecutor::new(provider.clone());
        let node = make_llm_node("primary-model", u32::MAX, None);
        // `attempts = 1 + max_retries` used to overflow-panic in debug builds;
        // the saturating computation must run and the first success must stop
        // the retry loop immediately.
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert!(
            matches!(result.state, NodeState::Succeeded),
            "immediate success must not panic on u32::MAX retries"
        );
        assert_eq!(
            provider.attempt_count(),
            1,
            "success on the first attempt consumes exactly one attempt"
        );
    }

    #[tokio::test]
    async fn test_retries_exhausted_then_fallback_model_used() {
        let provider = Arc::new(FailingProvider::new());
        let executor = ProviderExecutor::new(provider.clone());
        let node = make_llm_node("primary-model", 1, Some("fallback-model"));
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        // FailingProvider always fails, so node itself fails — but we must
        // verify the fallback model was attempted.
        let attempts = provider.fallback_attempts();
        assert!(
            attempts.contains(&"primary-model".to_string()),
            "primary model must be tried"
        );
        assert!(
            attempts.contains(&"fallback-model".to_string()),
            "fallback model must be tried after retries"
        );
        assert_eq!(attempts.len(), 3, "1 primary + 1 retry + 1 fallback");
        assert!(matches!(result.state, NodeState::Failed(_)));
    }

    #[tokio::test]
    async fn test_no_fallback_fails_after_retries() {
        let provider = Arc::new(FailingProvider::new());
        let executor = ProviderExecutor::new(provider.clone());
        let node = make_llm_node("primary-model", 1, None);
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert!(matches!(result.state, NodeState::Failed(_)));
        assert_eq!(
            provider.fallback_attempts().len(),
            2,
            "1 primary + 1 retry, no fallback"
        );
    }

    #[tokio::test]
    async fn test_fallback_success_when_primary_always_fails() {
        struct FallbackSucceedsProvider {
            attempts: std::sync::Mutex<Vec<String>>,
        }
        #[async_trait]
        impl ChatProvider for FallbackSucceedsProvider {
            async fn chat_completion(&self, request: &ChatRequest) -> Result<ChatResponse, String> {
                self.attempts.lock().unwrap().push(request.model.clone());
                if request.model == "fallback-model" {
                    Ok(ChatResponse {
                        content: "fallback answer".into(),
                        usage: Some(Usage {
                            prompt_tokens: 1,
                            completion_tokens: 1,
                            total_tokens: 2,
                        }),
                        tool_calls: vec![],
                        tool_results: Vec::new(),
                    })
                } else {
                    Err("primary down".into())
                }
            }
        }

        let provider = Arc::new(FallbackSucceedsProvider {
            attempts: std::sync::Mutex::new(Vec::new()),
        });
        let executor = ProviderExecutor::new(provider.clone());
        let node = make_llm_node("primary-model", 0, Some("fallback-model"));
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert!(
            matches!(result.state, NodeState::Succeeded),
            "fallback must save the node"
        );
        let attempts = provider.attempts.lock().unwrap().clone();
        assert_eq!(
            attempts,
            vec!["primary-model".to_string(), "fallback-model".to_string()]
        );
    }

    // -----------------------------------------------------------------------
    // Phase 4.3: subgraph execution tests
    // -----------------------------------------------------------------------

    /// Consensus-like subgraph: 2 member generates + 1 judge.
    /// The judge must receive both member outputs as parent context.
    fn make_consensus_subgraph() -> (ExecutionNode, Arc<SpyProvider>) {
        let spy = Arc::new(SpyProvider::new());
        let member_a = uuid::Uuid::new_v4();
        let member_b = uuid::Uuid::new_v4();
        let judge = uuid::Uuid::new_v4();
        let subgraph = ExecutionSubgraph {
            nodes: vec![
                ExecutionNode {
                    id: member_a,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "member_a".into(),
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: member_b,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "member_b".into(),
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: judge,
                    kind: ExecutionNodeKind::LLMJudge,
                    strategy: StrategyKind::Single,
                    model: "judge".into(),
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
            ],
            edges: vec![
                ExecutionEdge {
                    from: member_a,
                    to: judge,
                    condition: None,
                },
                ExecutionEdge {
                    from: member_b,
                    to: judge,
                    condition: None,
                },
            ],
            entry_node_id: member_a,
            exit_node_id: judge,
        };
        let node = ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Consensus,
            model: "consensus".into(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config: HashMap::new(),
            subgraph: Some(subgraph),
        };
        (node, spy)
    }

    #[tokio::test]
    async fn test_subgraph_executes_members_and_judge() {
        let (node, spy) = make_consensus_subgraph();
        let executor = ProviderExecutor::new(spy.clone());
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert!(
            matches!(result.state, NodeState::Succeeded),
            "subgraph must succeed"
        );
        let output = result.output.expect("output");
        assert_eq!(output["subgraph"], true);
        assert!(!output["exit_node_id"].as_str().unwrap().is_empty());

        // Sub-node outputs must be in graph_outputs when judge runs
        let judge_request = spy.last_request();
        assert!(judge_request.is_some(), "judge must be called");
        let judge_request = judge_request.unwrap();
        assert_eq!(judge_request.model, "judge");
        // Judge request must contain member context (parent output messages)
        let messages = &judge_request.messages;
        assert!(
            messages
                .iter()
                .any(|m| m.role == "user" && m.content.contains("Context from parent node")),
            "judge must see member outputs"
        );
        assert!(
            messages
                .iter()
                .any(|m| m.role == "system" && m.content.contains("judge")),
            "judge must have system prompt"
        );
    }

    #[tokio::test]
    async fn test_subgraph_failure_propagates() {
        let spy = Arc::new(FailingProvider::new());
        let (mut node, _) = make_consensus_subgraph();
        node.subgraph.as_mut().unwrap().nodes[0].retry_policy = RetryPolicy {
            max_retries: 0,
            backoff_ms: 0,
        };
        let executor = ProviderExecutor::new(spy);
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert!(
            matches!(result.state, NodeState::Failed(_)),
            "subgraph member failure must propagate"
        );
    }

    // -----------------------------------------------------------------------
    // Phase 4.4: control-flow kinds — Gate must not call provider
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_gate_node_does_not_call_provider() {
        let spy = Arc::new(SpyProvider::new());
        let node = ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::Gate,
            strategy: StrategyKind::Single,
            model: "policy.approval_gate".into(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config: HashMap::new(),
            subgraph: None,
        };
        let mut store = InMemoryApprovalStore::new();
        store.approve(node.id);
        let executor = ProviderExecutor::new(spy.clone()).with_approvals(Arc::new(store));
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert!(
            matches!(result.state, NodeState::Succeeded),
            "gate must succeed"
        );
        assert!(
            spy.last_request().is_none(),
            "gate must NOT call the LLM provider"
        );
    }

    // -----------------------------------------------------------------------
    // Phase 4.5: tools — fail closed by default
    // -----------------------------------------------------------------------

    /// Provider that returns a tool call on its first invocation, then a plain
    /// answer on subsequent invocations.
    struct ToolingProvider {
        calls: std::sync::Mutex<usize>,
    }

    impl ToolingProvider {
        fn new() -> Self {
            Self {
                calls: std::sync::Mutex::new(0),
            }
        }

        fn call_count(&self) -> usize {
            *self.calls.lock().unwrap()
        }
    }

    #[async_trait]
    impl ChatProvider for ToolingProvider {
        async fn chat_completion(&self, request: &ChatRequest) -> Result<ChatResponse, String> {
            let mut calls = self.calls.lock().unwrap();
            *calls += 1;
            if *calls == 1 {
                return Ok(ChatResponse {
                    content: String::new(),
                    usage: Some(Usage {
                        prompt_tokens: 10,
                        completion_tokens: 5,
                        total_tokens: 15,
                    }),
                    tool_calls: vec![ToolCall {
                        id: "call_1".into(),
                        name: "echo".into(),
                        arguments: serde_json::json!({ "text": "hi" }),
                    }],
                    tool_results: Vec::new(),
                });
            }
            Ok(ChatResponse {
                content: format!(
                    "final answer (tool shim messages: {})",
                    request.messages.len()
                ),
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
                tool_calls: vec![],
                tool_results: Vec::new(),
            })
        }
    }

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echoes back its text argument."
        }

        async fn execute(&self, arguments: serde_json::Value) -> Result<serde_json::Value, String> {
            Ok(serde_json::json!({ "echoed": arguments.get("text") }))
        }
    }

    fn tool_node(config: std::collections::HashMap<String, serde_json::Value>) -> ExecutionNode {
        ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: "tool-model".into(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config,
            subgraph: None,
        }
    }

    #[tokio::test]
    async fn test_tool_calls_fail_closed_when_auto_exec_disabled() {
        let executor = ProviderExecutor::new(Arc::new(ToolingProvider::new()));
        let node = tool_node(HashMap::new());
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert!(
            matches!(&result.state, NodeState::Failed(msg) if msg.contains("auto-exec")),
            "tool calls with auto-exec disabled must fail closed, got {:?}",
            result.state
        );
    }

    #[tokio::test]
    async fn test_tool_not_in_allowlist_fails_closed() {
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(EchoTool));
        let executor = ProviderExecutor::new(Arc::new(ToolingProvider::new()))
            .with_tools(tools)
            .with_allow_auto_exec(true);
        let node = tool_node(HashMap::new());
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert!(
            matches!(&result.state, NodeState::Succeeded),
            "deny must not abort the node, got {:?}",
            result.state
        );
        let output = result.output.expect("succeeded node must carry output");
        let entry = &output["tool_calls"][0];
        assert_eq!(
            entry["executed"],
            serde_json::json!(false),
            "tool must not execute"
        );
        assert_eq!(entry["error"], serde_json::json!(true));
        assert!(
            entry["reason"].as_str().unwrap().contains("allowlist"),
            "reason must cite the allowlist, got {}",
            entry["reason"]
        );
    }

    #[tokio::test]
    async fn test_tool_not_registered_fails_closed() {
        let executor =
            ProviderExecutor::new(Arc::new(ToolingProvider::new())).with_allow_auto_exec(true);
        let node = tool_node(HashMap::from([(
            "tool_allowlist".to_string(),
            serde_json::json!(["echo"]),
        )]));
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert!(
            matches!(&result.state, NodeState::Succeeded),
            "unregistered-tool deny must not abort the node, got {:?}",
            result.state
        );
        let output = result.output.expect("succeeded node must carry output");
        let entry = &output["tool_calls"][0];
        assert_eq!(
            entry["executed"],
            serde_json::json!(false),
            "tool must not execute"
        );
        assert_eq!(entry["error"], serde_json::json!(true));
        assert!(
            entry["reason"].as_str().unwrap().contains("not registered"),
            "reason must cite registration failure, got {}",
            entry["reason"]
        );
    }

    #[tokio::test]
    async fn test_tool_executes_when_allowlisted_and_registered() {
        let provider = Arc::new(ToolingProvider::new());
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(EchoTool));
        let executor = ProviderExecutor::new(provider.clone())
            .with_tools(tools)
            .with_allow_auto_exec(true);
        let node = tool_node(HashMap::from([(
            "tool_allowlist".to_string(),
            serde_json::json!(["echo"]),
        )]));
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert!(
            matches!(result.state, NodeState::Succeeded),
            "expected success, got {:?}",
            result.state
        );
        assert_eq!(
            provider.call_count(),
            2,
            "provider must be called twice (tool + final)"
        );
        let output = result.output.expect("output");
        assert!(
            output["content"]
                .as_str()
                .unwrap()
                .contains("tool shim messages: 1"),
            "second round-trip must include one tool-results message, got {:?}",
            output
        );
    }

    #[tokio::test]
    async fn test_max_tool_iterations_guard() {
        struct InfiniteToolProvider;

        #[async_trait]
        impl ChatProvider for InfiniteToolProvider {
            async fn chat_completion(
                &self,
                _request: &ChatRequest,
            ) -> Result<ChatResponse, String> {
                Ok(ChatResponse {
                    content: String::new(),
                    usage: None,
                    tool_calls: vec![ToolCall {
                        id: "call_1".into(),
                        name: "echo".into(),
                        arguments: serde_json::json!({}),
                    }],
                    tool_results: Vec::new(),
                })
            }
        }

        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(EchoTool));
        let executor = ProviderExecutor::new(Arc::new(InfiniteToolProvider))
            .with_tools(tools)
            .with_allow_auto_exec(true);
        let node = tool_node(HashMap::from([(
            "tool_allowlist".to_string(),
            serde_json::json!(["echo"]),
        )]));
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert!(
            matches!(&result.state, NodeState::Failed(msg) if msg.contains("max_tool_iterations")),
            "infinite tool loop must be capped, got {:?}",
            result.state
        );
    }

    // -----------------------------------------------------------------------
    // Phase 4.6: runtime budget envelope (light)
    // -----------------------------------------------------------------------

    fn make_chain_graph(
        models: &[&str],
        config: HashMap<String, serde_json::Value>,
    ) -> Arc<ExecutionGraph> {
        let node_ids: Vec<uuid::Uuid> = models.iter().map(|_| uuid::Uuid::new_v4()).collect();
        Arc::new(ExecutionGraph {
            graph_id: uuid::Uuid::new_v4(),
            nodes: node_ids
                .iter()
                .zip(models)
                .map(|(id, model)| ExecutionNode {
                    id: *id,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: (*model).into(),
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
                    fallback: None,
                    config: config.clone(),
                    subgraph: None,
                })
                .collect(),
            edges: node_ids
                .windows(2)
                .map(|w| ExecutionEdge {
                    from: w[0],
                    to: w[1],
                    condition: None,
                })
                .collect(),
            metadata: GraphMetadata {
                policy_version: 0,
                estimated_cost: NanoUSD::from_nanos(10_000_000),
                estimated_tokens: 200,
                max_depth: node_ids.len() as u32,
                node_count: node_ids.len() as u32,
            },
            total_tokens: 200,
            total_cost: NanoUSD::from_nanos(10),
            primitive_graph_hash: 0,
        })
    }

    #[tokio::test]
    async fn test_tight_quota_fails_mid_run() {
        use fusion_kernel::resource::{ResourceManager, StubResourceManager};

        let rm: Arc<dyn ResourceManager> =
            Arc::new(StubResourceManager::new(fusion_kernel::resource::Quota {
                max_daily_cost: NanoUSD::ONE_DOLLAR,
                max_daily_tokens: 100,
            }));
        let provider: Arc<dyn ChatProvider> = Arc::new(MockProvider::default_response());
        let engine = RuntimeEngine::new(provider).with_resource_manager(rm.clone());
        // Each node estimates 75 tokens; the stub records 75 actual tokens.
        // Node 1 passes (0+75 <= 100), node 2 busts the quota (75+75 > 100).
        let graph = make_chain_graph(
            &["n1", "n2"],
            HashMap::from([("budget_estimated_tokens".to_string(), serde_json::json!(75))]),
        );
        let outcome = engine.run(graph).await.expect("run");
        assert!(!outcome.success, "tight quota must fail the run");
        assert_eq!(outcome.outputs.len(), 1, "only n1 must succeed");
        assert_eq!(rm.spent_tokens(), 75, "only n1's tokens recorded");
    }

    #[tokio::test]
    async fn test_generous_quota_allows_run() {
        use fusion_kernel::resource::{ResourceManager, StubResourceManager};

        let rm: Arc<dyn ResourceManager> =
            Arc::new(StubResourceManager::new(fusion_kernel::resource::Quota {
                max_daily_cost: NanoUSD::ONE_DOLLAR,
                max_daily_tokens: 10_000,
            }));
        let provider: Arc<dyn ChatProvider> = Arc::new(MockProvider::default_response());
        let engine = RuntimeEngine::new(provider).with_resource_manager(rm.clone());
        let graph = make_chain_graph(
            &["n1", "n2"],
            HashMap::from([("budget_estimated_tokens".to_string(), serde_json::json!(75))]),
        );
        let outcome = engine.run(graph).await.expect("run");
        assert!(outcome.success);
        assert_eq!(outcome.outputs.len(), 2);
        assert_eq!(rm.spent_tokens(), 150, "both nodes' usage recorded");
    }

    #[tokio::test]
    async fn test_budget_error_reports_node_and_quota() {
        use fusion_kernel::resource::{ResourceManager, StubResourceManager};

        let rm: Arc<dyn ResourceManager> =
            Arc::new(StubResourceManager::new(fusion_kernel::resource::Quota {
                max_daily_cost: NanoUSD::ONE_DOLLAR,
                max_daily_tokens: 100,
            }));
        let executor = ProviderExecutor::new(Arc::new(MockProvider::default_response()))
            .with_resource_manager(rm);
        let node = ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: "n".into(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config: HashMap::from([(
                "budget_estimated_tokens".to_string(),
                serde_json::json!(101),
            )]),
            subgraph: None,
        };
        let result = executor
            .execute_node(&node, &NodeExecContext::default())
            .await;
        assert!(
            matches!(&result.state, NodeState::Failed(msg) if msg.contains("budget exceeded")),
            "quota miss must fail the node with a clear message, got {:?}",
            result.state
        );
    }

    // -----------------------------------------------------------------------
    // Phase 4.7: E2E golden extensions (engine-level)
    // -----------------------------------------------------------------------

    /// Provider that records every request it receives.
    struct RecordingProvider {
        requests: std::sync::Mutex<Vec<ChatRequest>>,
    }

    impl RecordingProvider {
        fn new() -> Self {
            Self {
                requests: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn requests(&self) -> Vec<ChatRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ChatProvider for RecordingProvider {
        async fn chat_completion(&self, request: &ChatRequest) -> Result<ChatResponse, String> {
            self.requests.lock().unwrap().push(request.clone());
            Ok(ChatResponse {
                content: format!("mock response for model {}", request.model),
                usage: Some(Usage {
                    prompt_tokens: 50,
                    completion_tokens: 25,
                    total_tokens: 75,
                }),
                tool_calls: vec![],
                tool_results: Vec::new(),
            })
        }
    }

    /// Golden 4.7.1: a balanced chain — every node after the first must see
    /// prior outputs in its request messages (parent propagation E2E).
    #[tokio::test]
    async fn test_golden_parent_chain_context_propagation() {
        let provider = Arc::new(RecordingProvider::new());
        let engine = RuntimeEngine::new(provider.clone());
        let graph = make_chain_graph(&["n1", "n2", "n3"], HashMap::new());
        let outcome = engine.run(graph).await.expect("run");
        assert!(outcome.success);
        assert_eq!(outcome.outputs.len(), 3);

        let requests = provider.requests();
        assert_eq!(requests.len(), 3, "each of 3 nodes calls the provider once");
        let n2 = requests
            .iter()
            .find(|r| r.model == "n2")
            .expect("n2 request");
        let n3 = requests
            .iter()
            .find(|r| r.model == "n3")
            .expect("n3 request");
        assert!(
            n2.messages
                .iter()
                .any(|m| m.content.contains("mock response for model n1")),
            "n2 must see n1's output, got {:?}",
            n2.messages
        );
        assert!(
            n3.messages
                .iter()
                .any(|m| m.content.contains("mock response for model n2")),
            "n3 must see n2's output, got {:?}",
            n3.messages
        );
    }

    /// Golden 4.7.2: flaky mock — run succeeds through retries.
    #[tokio::test]
    async fn test_golden_retry_recovers_mid_run() {
        let provider = Arc::new(FlakyProvider::new(2));
        let engine = RuntimeEngine::new(provider.clone());
        let n1 = uuid::Uuid::new_v4();
        let graph = Arc::new(ExecutionGraph {
            graph_id: uuid::Uuid::new_v4(),
            nodes: vec![ExecutionNode {
                id: n1,
                kind: ExecutionNodeKind::LLMGenerate,
                strategy: StrategyKind::Single,
                model: "flaky".into(),
                retry_policy: RetryPolicy {
                    max_retries: 3,
                    backoff_ms: 0,
                },
                fallback: None,
                config: HashMap::new(),
                subgraph: None,
            }],
            edges: vec![],
            metadata: GraphMetadata {
                policy_version: 0,
                estimated_cost: NanoUSD::from_nanos(10_000_000),
                estimated_tokens: 100,
                max_depth: 1,
                node_count: 1,
            },
            total_tokens: 100,
            total_cost: NanoUSD::from_nanos(10),
            primitive_graph_hash: 0,
        });
        let outcome = engine.run(graph).await.expect("run");
        assert!(outcome.success, "retries must recover from flaky provider");
        assert_eq!(provider.attempt_count(), 3, "2 failures + 1 success");
    }

    /// Golden 4.7.3: approval Gate in the path — run succeeds without an LLM
    /// call on the gate node.
    #[tokio::test]
    async fn test_golden_gate_path_runs_without_llm_on_gate() {
        let provider = Arc::new(SpyProvider::new());
        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();
        let mut store = InMemoryApprovalStore::new();
        store.approve(n2);
        let engine = RuntimeEngine::new(provider.clone()).with_approvals(Arc::new(store));
        let graph = Arc::new(ExecutionGraph {
            graph_id: uuid::Uuid::new_v4(),
            nodes: vec![
                ExecutionNode {
                    id: n1,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "gen".into(),
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: n2,
                    kind: ExecutionNodeKind::Gate,
                    strategy: StrategyKind::Single,
                    model: "policy.approval_gate".into(),
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
                    fallback: None,
                    config: HashMap::from([(
                        "approval_policy".to_string(),
                        serde_json::json!("requires_review"),
                    )]),
                    subgraph: None,
                },
            ],
            edges: vec![ExecutionEdge {
                from: n1,
                to: n2,
                condition: None,
            }],
            metadata: GraphMetadata {
                policy_version: 0,
                estimated_cost: NanoUSD::ZERO,
                estimated_tokens: 0,
                max_depth: 2,
                node_count: 2,
            },
            total_tokens: 0,
            total_cost: NanoUSD::ZERO,
            primitive_graph_hash: 0,
        });
        let outcome = engine.run(graph).await.expect("run");
        assert!(outcome.success, "gate must not fail the run");
        let last = provider
            .last_request()
            .expect("generation node must call provider");
        assert_eq!(last.model, "gen", "gate must NOT call the LLM provider");
    }

    /// Golden 4.7.4: hand-built multi-member subgraph — judge output present
    /// in the final node output.
    #[tokio::test]
    async fn test_golden_subgraph_judge_output_present() {
        let provider = Arc::new(RecordingProvider::new());
        let engine = RuntimeEngine::new(provider.clone());
        let (node, _) = make_consensus_subgraph();
        let graph = Arc::new(ExecutionGraph {
            graph_id: uuid::Uuid::new_v4(),
            nodes: vec![node],
            edges: vec![],
            metadata: GraphMetadata {
                policy_version: 0,
                estimated_cost: NanoUSD::ZERO,
                estimated_tokens: 0,
                max_depth: 1,
                node_count: 1,
            },
            total_tokens: 0,
            total_cost: NanoUSD::ZERO,
            primitive_graph_hash: 0,
        });
        let outcome = engine.run(graph).await.expect("run");
        assert!(outcome.success);
        let output = outcome.outputs.values().next().expect("one node output");
        assert_eq!(output.get("subgraph").and_then(|v| v.as_bool()), Some(true));
        let exit_node_id = output
            .get("exit_node_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            !exit_node_id.is_empty(),
            "judge (exit node) id must be present"
        );
    }

    /// Golden 4.7.5: tool deny-by-default — a tool call is never executed when
    /// auto-exec is off, even if allowlisted.
    #[tokio::test]
    async fn test_golden_tool_deny_by_default() {
        let provider: Arc<dyn ChatProvider> = Arc::new(ToolingProvider::new());
        let engine = RuntimeEngine::new(provider); // auto_exec off (default)
        let graph = make_chain_graph(
            &["tool-node"],
            HashMap::from([("tool_allowlist".to_string(), serde_json::json!(["echo"]))]),
        );
        let outcome = engine.run(graph).await.expect("run");
        assert!(
            !outcome.success,
            "tool call must fail closed when auto-exec off"
        );
    }
}
