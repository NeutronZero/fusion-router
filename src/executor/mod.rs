use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use tracing::info;

pub mod capability_executor;

#[cfg(feature = "semantic-cache")]
use crate::cache::SemanticCache;
use crate::compiler::context::CompilationContext;
use crate::compiler::ir::StrategyIR;
use crate::providers::ChatProvider;
use crate::strategies::Strategy;
use crate::tools::ToolRegistry;
use crate::types::{
    ChatCompletionRequest, ChatMessage, ExecutionGraph, ExecutionNode, ExecutionNodeKind,
    ExecutionSubgraph, NodeExecutionResult, NodeState, StrategyKind, ToolCall, ToolDefinition, Usage,
};

#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute_node(&self, node: &ExecutionNode) -> NodeExecutionResult;
    async fn resolve_strategy(&self, node: &ExecutionNode) -> ExecutionSubgraph;
}

pub struct DefaultExecutor {
    pub provider: Arc<dyn ChatProvider + Send + Sync>,
    pub strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>>,
    #[cfg(feature = "semantic-cache")]
    pub cache: Option<Arc<SemanticCache>>,
    pub tool_registry: Option<Arc<ToolRegistry>>,
    /// Law 7 / ADR-037: when false (default), provider-native `tool_calls`
    /// are surfaced as text and NEVER executed.
    pub allow_auto_exec: bool,
}

impl DefaultExecutor {
    pub fn new(
        provider: Arc<dyn ChatProvider + Send + Sync>,
        strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>>,
    ) -> Self {
        Self {
            provider,
            strategies,
            #[cfg(feature = "semantic-cache")]
            cache: None,
            tool_registry: None,
            allow_auto_exec: false,
        }
    }

    #[cfg(feature = "semantic-cache")]
    pub fn with_cache(mut self, cache: Arc<SemanticCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn with_tool_registry(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    pub fn with_allow_auto_exec(mut self, allow: bool) -> Self {
        self.allow_auto_exec = allow;
        self
    }

    /// Request-scoped tool allowlist from the node config. An absent or
    /// empty allowlist means NO tool may execute (fail closed).
    fn request_tool_allowlist(node: &ExecutionNode) -> Vec<String> {
        node.config
            .get("tool_allowlist")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Tool definitions are advertised to the provider ONLY when auto
    /// execution is enabled AND the request names an allowlist. Otherwise no
    /// definitions are sent, so the provider cannot emit tool calls at all.
    fn request_tool_definitions(&self, node: &ExecutionNode) -> Option<Vec<ToolDefinition>> {
        if !self.allow_auto_exec {
            return None;
        }
        let registry = self.tool_registry.as_ref()?;
        let allowlist = Self::request_tool_allowlist(node);
        if allowlist.is_empty() {
            return None;
        }
        let defs: Vec<ToolDefinition> = allowlist
            .iter()
            .filter_map(|name| {
                let tool = registry.get(name)?;
                Some(ToolDefinition {
                    name: tool.name().to_string(),
                    description: tool.description().to_string(),
                    parameters: Some(tool.schema()),
                })
            })
            .collect();
        if defs.is_empty() { None } else { Some(defs) }
    }

    /// Law 7 / ADR-037: executes provider-native tool calls under the
    /// per-request allowlist. Calls that are not allowlisted, or that arrive
    /// while auto-execution is disabled, are returned as text — never run.
    async fn execute_native_tool_calls(
        &self,
        node: &ExecutionNode,
        calls: &[ToolCall],
    ) -> serde_json::Value {
        let allowlist = Self::request_tool_allowlist(node);
        let mut results = Vec::with_capacity(calls.len());
        for call in calls {
            let allowlisted = self.allow_auto_exec
                && !allowlist.is_empty()
                && allowlist.iter().any(|t| t == &call.name);
            let registry_has = self
                .tool_registry
                .as_ref()
                .map(|r| r.contains(&call.name))
                .unwrap_or(false);

            if allowlisted && registry_has {
                // Registry presence was checked above; avoid panics anyway if
                // the registry changed between the check and the lookup.
                let Some(registry) = self.tool_registry.as_ref() else {
                    results.push(serde_json::json!({
                        "id": call.id,
                        "tool": call.name,
                        "arguments": call.arguments,
                        "error": "tool registry unavailable",
                        "executed": false,
                    }));
                    continue;
                };
                let Some(tool) = registry.get(&call.name) else {
                    results.push(serde_json::json!({
                        "id": call.id,
                        "tool": call.name,
                        "arguments": call.arguments,
                        "error": "tool not registered",
                        "executed": false,
                    }));
                    continue;
                };
                match tool.execute(call.arguments.clone()).await
                {
                    Ok(result) => {
                        info!(tool = %call.name, "Tool executed successfully (native tool call)");
                        results.push(serde_json::json!({
                            "id": call.id,
                            "tool": call.name,
                            "arguments": call.arguments,
                            "result": result,
                            "executed": true,
                        }));
                    }
                    Err(e) => {
                        info!(tool = %call.name, error = %e, "Tool execution failed");
                        results.push(serde_json::json!({
                            "id": call.id,
                            "tool": call.name,
                            "arguments": call.arguments,
                            "error": e,
                            "executed": false,
                        }));
                    }
                }
            } else {
                info!(
                    tool = %call.name,
                    auto_exec = self.allow_auto_exec,
                    "Tool call not executed: outside per-request allowlist"
                );
                results.push(serde_json::json!({
                    "id": call.id,
                    "tool": call.name,
                    "arguments": call.arguments,
                    "executed": false,
                    "reason": "tool not allowed by per-request allowlist",
                }));
            }
        }
        serde_json::json!({ "tool_calls": results })
    }

    /// Belt-and-suspenders: strategy sub-nodes built at compile time never
    /// carry the request's assembled messages (the pipeline only injects them
    /// into top-level nodes). Copy the parent node's messages (and the
    /// per-request tool allowlist, when present) into any LLM sub-node that
    /// lacks them so requests never go out with an empty `messages` array and
    /// tool definitions remain available for sub-node dispatch.
    fn propagate_parent_messages(node: &ExecutionNode, subgraph: &mut ExecutionSubgraph) {
        let Some(messages) = node
            .config
            .get("messages")
            .filter(|v| v.as_array().map(|a| !a.is_empty()).unwrap_or(false))
            .cloned()
        else {
            return;
        };
        let tool_allowlist = node.config.get("tool_allowlist").cloned();
        for sub_node in &mut subgraph.nodes {
            if !matches!(sub_node.kind, ExecutionNodeKind::LLMGenerate | ExecutionNodeKind::LLMReview | ExecutionNodeKind::LLMJudge) {
                continue;
            }
            let has_messages = sub_node
                .config
                .get("messages")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            if !has_messages {
                sub_node.config.insert("messages".to_string(), messages.clone());
            }
            if sub_node.config.get("tool_allowlist").is_none() {
                if let Some(ref allowlist) = tool_allowlist {
                    sub_node.config.insert("tool_allowlist".to_string(), allowlist.clone());
                }
            }
        }
    }

    #[tracing::instrument(skip_all, fields(node_id = %node.id, model = %node.model))]
    fn build_request(&self, node: &ExecutionNode) -> ChatCompletionRequest {
        let mut messages: Vec<ChatMessage> = node
            .config
            .get("messages")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

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

        // Inject system prompt for judge/reflect nodes
        if !messages.iter().any(|m| m.role == "system") {
            let system_prompt = match node.kind {
                ExecutionNodeKind::LLMJudge => Some("You are a judge evaluating the quality and correctness of responses. Assess the provided answers critically and select the best one, explaining your reasoning."),
                _ => match node.strategy {
                    crate::types::StrategyKind::Reflection => Some("You are a reflective reviewer. Analyze the previous response, identify potential issues, and provide an improved version."),
                    _ => None,
                },
            };
            if let Some(prompt) = system_prompt {
                messages.insert(0, ChatMessage {
                    role: "system".to_string(),
                    content: prompt.to_string(),
                });
            }
        }

        ChatCompletionRequest {
            model: node.model.clone(),
            messages,
            stream: false,
            temperature,
            max_tokens,
            tools: self.request_tool_definitions(node),
            files: None,
            execution: None,
            output: None,
            strategy: None,
        }
    }

    #[cfg(feature = "semantic-cache")]
    fn cache_key(request: &ChatCompletionRequest) -> String {
        let messages_json = serde_json::to_string(&request.messages).unwrap_or_default();
        format!("{}:{}", request.model, messages_json)
    }
}

#[async_trait]
impl Executor for DefaultExecutor {
    #[tracing::instrument(skip(self, node), fields(node_id = %node.id, model = %node.model, kind = ?node.kind, strategy = ?node.strategy))]
    async fn execute_node(&self, node: &ExecutionNode) -> NodeExecutionResult {
        let start = std::time::Instant::now();
        let strategy_label = format!("{:?}", node.strategy);
        let subgraph = self.resolve_strategy(node).await;
        let mut accumulated_usage: Option<Usage> = None;
        let mut output_value: Option<serde_json::Value> = None;

        tracing::debug!(
            strategy = %strategy_label,
            subgraph_nodes = subgraph.nodes.len(),
            "strategy execution started"
        );

        // Topologically order nodes by their edges so every node executes
        // after its dependencies. Cycles or disconnected nodes fall back to
        // insertion order.
        let mut incoming: HashMap<uuid::Uuid, Vec<uuid::Uuid>> = HashMap::new();
        for edge in &subgraph.edges {
            incoming.entry(edge.to).or_default().push(edge.from);
        }
        let mut remaining: Vec<&ExecutionNode> = subgraph.nodes.iter().collect();
        let mut completed: std::collections::HashSet<uuid::Uuid> = std::collections::HashSet::new();
        let mut order: Vec<&ExecutionNode> = Vec::with_capacity(subgraph.nodes.len());
        while !remaining.is_empty() {
            let ready: Vec<&ExecutionNode> = remaining
                .iter()
                .filter(|n| {
                    incoming
                        .get(&n.id)
                        .is_none_or(|froms| froms.iter().all(|f| completed.contains(f)))
                })
                .copied()
                .collect();
            if ready.is_empty() {
                order.extend(remaining.iter().copied());
                break;
            }
            for n in &ready {
                completed.insert(n.id);
                order.push(n);
            }
            remaining.retain(|n| !completed.contains(&n.id));
        }

        // Per-node outputs, keyed by node id, so judge/reducer nodes can
        // consume the outputs of their upstream members.
        let mut node_outputs: HashMap<uuid::Uuid, serde_json::Value> = HashMap::new();

        for sub_node in order {
            match sub_node.kind {
                ExecutionNodeKind::LLMGenerate
                | ExecutionNodeKind::LLMReview
                | ExecutionNodeKind::LLMJudge => {
                    let mut request = self.build_request(sub_node);
                    // Judges must see the output of every upstream member.
                    if sub_node.kind == ExecutionNodeKind::LLMJudge {
                        if let Some(froms) = incoming.get(&sub_node.id) {
                            for from in froms {
                                if let Some(out) = node_outputs.get(from) {
                                    request.messages.push(ChatMessage {
                                        role: "user".to_string(),
                                        content: format!("Member output:\n{}", out),
                                    });
                                }
                            }
                        }
                        tracing::debug!(
                            node_id = %sub_node.id,
                            message_count = request.messages.len(),
                            roles = ?request.messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(),
                            "judge request assembled"
                        );
                    }
                    #[cfg(feature = "semantic-cache")]
                    let cache_key = Self::cache_key(&request);

                    #[cfg(feature = "semantic-cache")]
                    if let Some(ref cache) = self.cache {
                        if let Some(cached) = cache.get(&cache_key).await {
                            info!(
                                node_id = %sub_node.id,
                                "Cache hit for LLM node"
                            );
                            let cached_output = cached
                                .get("content")
                                .and_then(|v| v.as_str())
                                .map(|s| serde_json::Value::String(s.to_string()))
                                .unwrap_or(cached);
                            // A cache hit satisfies just this sub-node: record
                            // it as the member output and continue with the
                            // remaining subgraph (other members, judge, exit
                            // node) instead of returning for the whole
                            // strategy.
                            output_value = Some(cached_output);
                            if let Some(current) = output_value.clone() {
                                node_outputs.insert(sub_node.id, current);
                            }
                            continue;
                        }
                    }

                    // Bounded tool loop (Law 7 / ADR-037): after executing
                    // provider-native tool calls the results are appended to
                    // the conversation and the model is re-prompted, so a
                    // review/workflow can read files, observe results, and
                    // continue. The loop ends when the model emits plain text
                    // or when the round budget is exhausted.
                    let max_tool_rounds = sub_node
                        .config
                        .get("max_tool_rounds")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(8)
                        .max(1);
                    let mut tool_round: u64 = 0;

                    loop {
                        let response = match self.provider.chat_completion(&request).await {
                            Ok(response) => response,
                            Err(e) => {
                                info!(
                                    node_id = %sub_node.id,
                                    error = %e,
                                    "LLM node failed"
                                );
                                let latency = start.elapsed().as_millis() as u64;
                                crate::telemetry::metrics::FusionMetrics::instance()
                                    .strategy_errors_total
                                    .with_label_values(&[&strategy_label])
                                    .inc();
                                crate::telemetry::metrics::FusionMetrics::instance()
                                    .strategy_latency_seconds
                                    .with_label_values(&[&strategy_label])
                                    .observe(latency as f64 / 1000.0);
                                return NodeExecutionResult {
                                    state: NodeState::Failed(format!("Provider error: {}", e)),
                                    usage: accumulated_usage,
                                    latency_ms: latency,
                                    output: None,
                                };
                            }
                        };

                        info!(
                            node_id = %sub_node.id,
                            model = %response.model,
                            tool_round,
                            request_messages = request.messages.len(),
                            response_content_len = response.choices.first()
                                .map(|c| c.message.content.len())
                                .unwrap_or(0),
                            has_tool_calls = response.native_tool_calls.as_ref().map(|c| c.len()).unwrap_or(0),
                            "LLM node completed"
                        );

                        output_value = response.choices.first()
                            .map(|c| c.message.content.clone())
                            .map(serde_json::Value::String);

                        if let Some(usage) = response.usage.clone() {
                            accumulated_usage = Some(match accumulated_usage {
                                Some(acc) => Usage {
                                    prompt_tokens: acc.prompt_tokens + usage.prompt_tokens,
                                    completion_tokens: acc.completion_tokens + usage.completion_tokens,
                                    total_tokens: acc.total_tokens + usage.total_tokens,
                                },
                                None => usage,
                            });
                        }

                        let wants_tools = response
                            .native_tool_calls
                            .as_ref()
                            .map(|c| !c.is_empty())
                            .unwrap_or(false);

                        if !wants_tools {
                            break;
                        }

                        // Law 7 / ADR-037: tool execution is fed ONLY from
                        // provider-native tool_calls. Model output text is
                        // never parsed for tool invocation.
                        let native_calls = response.native_tool_calls.clone().unwrap_or_default();
                        let results = self
                            .execute_native_tool_calls(sub_node, &native_calls)
                            .await;
                        let executed_any = results["tool_calls"]
                            .as_array()
                            .map(|c| c.iter().any(|x| x["executed"] == true))
                            .unwrap_or(false);

                        if !executed_any || tool_round + 1 >= max_tool_rounds {
                            // Nothing ran (e.g. all calls outside the
                            // allowlist) or the round budget is exhausted:
                            // stop. If the model produced a final text answer
                            // (possibly alongside the tool calls), keep it;
                            // otherwise surface the raw tool results so the
                            // stop does not discard information.
                            let has_text = output_value
                                .as_ref()
                                .and_then(|v| v.as_str())
                                .map(|s| !s.trim().is_empty())
                                .unwrap_or(false);
                            if !has_text {
                                output_value = Some(results);
                            }
                            break;
                        }

                        // Feed the results back and let the model continue.
                        request.messages.push(ChatMessage {
                            role: "user".to_string(),
                            content: format!("Tool results:\n{}", results),
                        });
                        tool_round += 1;
                        tracing::debug!(
                            node_id = %sub_node.id,
                            tool_round,
                            "tool round completed; continuing LLM loop"
                        );
                    }; // tool loop

                    #[cfg(feature = "semantic-cache")]
                    if let Some(ref cache) = self.cache {
                        // Only cache textual answers; JSON tool-result payloads
                        // or empty output must never pollute the cache.
                        if let Some(content) = output_value.as_ref().and_then(|v| v.as_str()) {
                            if !content.trim().is_empty() {
                                cache.put(&cache_key, serde_json::json!({ "content": content })).await;
                            }
                        }
                    }

                    if let Some(current) = output_value.clone() {
                        node_outputs.insert(sub_node.id, current);
                    }
                }
                ExecutionNodeKind::Transform
                | ExecutionNodeKind::Gate
                | ExecutionNodeKind::Conditional
                | ExecutionNodeKind::Loop
                | ExecutionNodeKind::Split
                | ExecutionNodeKind::Join
                | ExecutionNodeKind::Barrier => {}
            }
        }

        // The strategy result is the exit node's output (e.g. the judge in a
        // consensus subgraph), not necessarily the last node that happened to
        // run.
        if subgraph.exit_node_id != node.id {
            if let Some(exit_output) = node_outputs.get(&subgraph.exit_node_id) {
                output_value = Some(exit_output.clone());
            }
        }

        let latency = start.elapsed().as_millis() as u64;
        tracing::debug!(
            strategy = %strategy_label,
            latency_ms = latency,
            success = true,
            "strategy execution completed"
        );
        crate::telemetry::metrics::FusionMetrics::instance()
            .strategy_latency_seconds
            .with_label_values(&[&strategy_label])
            .observe(latency as f64 / 1000.0);
        NodeExecutionResult {
            state: NodeState::Succeeded,
            usage: accumulated_usage,
            latency_ms: latency,
            output: output_value,
        }
    }

    #[tracing::instrument(skip(self, node), fields(node_id = %node.id, strategy = ?node.strategy))]
    async fn resolve_strategy(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        // Compile-time expansion (production path): `lower_to_graph` attaches
        // the prebuilt subgraph; the executor executes it as-is. Runtime
        // lowering below is only a fallback for legacy graphs that were not
        // compiled through `build_compiler`.
        if let Some(prebuilt) = &node.subgraph {
            let mut subgraph = prebuilt.clone();
            Self::propagate_parent_messages(node, &mut subgraph);
            return subgraph;
        }

        let strategy = self.strategies.get(&node.strategy);
        if let Some(s) = strategy {
            let mut ctx = CompilationContext::new();
            if !node.model.is_empty() {
                ctx.available_models.push(node.model.clone());
            }
            let ir = strategy_ir_from_node(node);
            match s.lower(&ir, &ctx) {
                Ok(pg) => {
                    let eg = pg.to_execution_graph(
                        node.strategy.clone(),
                        &node.retry_policy,
                        &node.fallback,
                        &node.config,
                    );
                    let mut subgraph = execution_graph_to_subgraph(&eg, node);
                    Self::propagate_parent_messages(node, &mut subgraph);
                    return subgraph;
                }
                Err(e) => {
                    tracing::warn!(
                        node_id = %node.id,
                        strategy = ?node.strategy,
                        error = %e,
                        "strategy lowering failed, falling back to passthrough"
                    );
                }
            }
            ExecutionSubgraph {
                nodes: vec![node.clone()],
                edges: vec![],
                entry_node_id: node.id,
                exit_node_id: node.id,
            }
        } else {
            info!(
                node_id = %node.id,
                strategy = ?node.strategy,
                "No strategy registered, using passthrough"
            );
            ExecutionSubgraph {
                nodes: vec![node.clone()],
                edges: vec![],
                entry_node_id: node.id,
                exit_node_id: node.id,
            }
        }
    }
}

fn strategy_ir_from_node(node: &ExecutionNode) -> StrategyIR {
    crate::compiler::strategy_expansion::strategy_ir_from_node(node)
}

fn execution_graph_to_subgraph(eg: &ExecutionGraph, template: &ExecutionNode) -> ExecutionSubgraph {
    let entry_id = eg.nodes.first().map(|n| n.id).unwrap_or(template.id);
    let exit_id = eg.nodes.last().map(|n| n.id).unwrap_or(template.id);

    ExecutionSubgraph {
        nodes: eg.nodes.clone(),
        edges: eg.edges.clone(),
        entry_node_id: entry_id,
        exit_node_id: exit_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::consensus::ConsensusStrategy;
    use crate::strategies::single::SingleStrategy;
    use crate::types::*;
    use std::collections::HashMap;
    use uuid::Uuid;

    struct MockChatProvider;

    #[async_trait]
    impl ChatProvider for MockChatProvider {
        async fn chat_completion(
            &self,
            _request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                id: "mock".into(),
                object: "chat.completion".into(),
                created: 0,
                model: "mock-model".into(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "mock response".into(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 20,
                    total_tokens: 30,
                }),
            })
        }

        fn name(&self) -> &str {
            "mock"
        }
    }

    struct CapturingMockProvider(Arc<std::sync::Mutex<Option<ChatCompletionRequest>>>);

    #[async_trait]
    impl ChatProvider for CapturingMockProvider {
        async fn chat_completion(
            &self,
            request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            *self.0.lock().unwrap() = Some(request.clone());
            Ok(ChatCompletionResponse {
                id: "mock".into(),
                object: "chat.completion".into(),
                created: 0,
                model: request.model.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "mock response".into(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 20,
                    total_tokens: 30,
                }),
            })
        }

        fn name(&self) -> &str {
            "capturing"
        }
    }

    fn make_llm_node(strategy: StrategyKind) -> ExecutionNode {
        ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy,
            model: "gpt-4".to_string(),
            retry_policy: RetryPolicy {
                max_retries: 3,
                backoff_ms: 1000,
            },
            fallback: None,
            config: HashMap::new(),
            subgraph: None,
        }
    }

    fn make_judge_node(strategy: StrategyKind) -> ExecutionNode {
        ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMJudge,
            strategy,
            model: "gpt-4".to_string(),
            retry_policy: RetryPolicy {
                max_retries: 3,
                backoff_ms: 1000,
            },
            fallback: None,
            config: HashMap::new(),
            subgraph: None,
        }
    }

    #[tokio::test]
    async fn test_debate_string_roles_lower_to_real_subgraph() {
        let provider = Arc::new(MockChatProvider);
        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(
            StrategyKind::Debate,
            Box::new(crate::strategies::debate::DebateStrategy {
                debaters: vec![Box::new(SingleStrategy), Box::new(SingleStrategy)],
                judge: Box::new(SingleStrategy),
            }),
        );
        let executor = DefaultExecutor::new(provider, strategies);
        let mut node = make_llm_node(StrategyKind::Debate);
        node.config.insert(
            "roles".into(),
            serde_json::json!(["Engineer A", "Engineer B"]),
        );

        let subgraph = executor.resolve_strategy(&node).await;

        assert_eq!(
            subgraph.nodes.len(),
            3,
            "2 string roles + judge must lower to 3 nodes, not passthrough"
        );
        assert!(
            subgraph.nodes.iter().all(|n| n.model == "gpt-4"),
            "string roles must inherit the workflow node's model"
        );
    }

    #[tokio::test]
    async fn test_resolve_strategy_consensus_subgraph_inherits_node_model() {
        let provider = Arc::new(MockChatProvider);
        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(StrategyKind::Consensus, Box::new(ConsensusStrategy::default()));
        let executor = DefaultExecutor::new(provider, strategies);
        let mut node = make_llm_node(StrategyKind::Consensus);
        node.model = "gpt-4-turbo".into();

        let subgraph = executor.resolve_strategy(&node).await;

        assert_eq!(subgraph.nodes.len(), 4);
        assert!(
            subgraph.nodes.iter().all(|n| n.model == "gpt-4-turbo"),
            "subgraph nodes must inherit the workflow node's model, got: {:?}",
            subgraph.nodes.iter().map(|n| &n.model).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn test_resolve_strategy_propagates_parent_messages_to_subnodes() {
        let provider = Arc::new(MockChatProvider);
        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(StrategyKind::Consensus, Box::new(ConsensusStrategy::default()));
        let executor = DefaultExecutor::new(provider, strategies);
        let mut node = make_llm_node(StrategyKind::Consensus);
        node.config.insert(
            "messages".into(),
            serde_json::json!([{ "role": "user", "content": "analyze the repo" }]),
        );

        let subgraph = executor.resolve_strategy(&node).await;

        assert_eq!(subgraph.nodes.len(), 4);
        for sub_node in &subgraph.nodes {
            let messages = sub_node
                .config
                .get("messages")
                .and_then(|v| v.as_array())
                .expect("LLM sub-node must inherit parent messages");
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0]["role"], "user");
            assert_eq!(messages[0]["content"], "analyze the repo");
        }
    }

    #[tokio::test]
    async fn test_execute_node_single_strategy() {
        let provider = Arc::new(MockChatProvider);
        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(StrategyKind::Single, Box::new(SingleStrategy));
        let executor = DefaultExecutor::new(provider, strategies);
        let node = make_llm_node(StrategyKind::Single);

        let result = executor.execute_node(&node).await;

        assert_eq!(result.state, NodeState::Succeeded);
        assert_eq!(
            result.output,
            Some(serde_json::Value::String("mock response".into()))
        );
        let usage = result.usage.expect("usage should be present");
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[tokio::test]
    async fn test_execute_node_strategy_fallback() {
        let provider = Arc::new(MockChatProvider);
        let strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        let executor = DefaultExecutor::new(provider, strategies);
        let node = make_llm_node(StrategyKind::Fusion);

        let result = executor.execute_node(&node).await;

        assert_eq!(result.state, NodeState::Succeeded);
    }

    #[tokio::test]
    async fn test_resolve_strategy_single() {
        let provider = Arc::new(MockChatProvider);
        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(StrategyKind::Single, Box::new(SingleStrategy));
        let executor = DefaultExecutor::new(provider, strategies);
        let node = make_llm_node(StrategyKind::Single);

        let subgraph = executor.resolve_strategy(&node).await;

        assert_eq!(subgraph.nodes.len(), 1);
        assert!(matches!(subgraph.nodes[0].kind, ExecutionNodeKind::LLMGenerate));
    }

    #[tokio::test]
    async fn test_resolve_strategy_consensus() {
        let provider = Arc::new(MockChatProvider);
        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(StrategyKind::Consensus, Box::new(ConsensusStrategy::default()));
        let executor = DefaultExecutor::new(provider, strategies);
        let node = make_llm_node(StrategyKind::Consensus);

        let subgraph = executor.resolve_strategy(&node).await;

        assert_eq!(subgraph.nodes.len(), 4);
        assert!(matches!(subgraph.nodes[0].kind, ExecutionNodeKind::LLMGenerate));
        assert!(matches!(
            subgraph.nodes.last().unwrap().kind,
            ExecutionNodeKind::LLMJudge
        ));
    }

    #[tokio::test]
    async fn test_build_request_injects_system_prompt() {
        let captured = Arc::new(std::sync::Mutex::new(None::<ChatCompletionRequest>));
        let provider = Arc::new(CapturingMockProvider(captured.clone()));
        let strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        let executor = DefaultExecutor::new(provider, strategies);
        let node = make_judge_node(StrategyKind::Fusion);

        let _ = executor.execute_node(&node).await;

        let request = captured.lock().unwrap().take().unwrap();
        let has_system = request.messages.iter().any(|m| m.role == "system");
        assert!(has_system, "expected a system message to be injected");
        let first_role = &request.messages[0].role;
        assert_eq!(first_role, "system", "system message should be first");
        assert!(
            request.messages[0].content.contains("judge"),
            "system prompt should reference 'judge' role"
        );
    }

    /// Captures ALL requests (not just the last) so we can verify
    /// which node received what input.
    struct CapturingAllProvider(Arc<std::sync::Mutex<Vec<ChatCompletionRequest>>>);

    #[async_trait]
    impl ChatProvider for CapturingAllProvider {
        async fn chat_completion(
            &self,
            request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            let mut seen = self.0.lock().unwrap();
            seen.push(request.clone());
            let idx = seen.len();
            Ok(ChatCompletionResponse {
                id: "mock".into(),
                object: "chat.completion".into(),
                created: 0,
                model: request.model.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: format!("response-{}", idx),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 20,
                    total_tokens: 30,
                }),
            })
        }

        fn name(&self) -> &str {
            "capturing-all"
        }
    }

    #[tokio::test]
    async fn test_consensus_judge_sees_member_outputs() {
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        let provider = Arc::new(CapturingAllProvider(captured.clone()));
        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(StrategyKind::Consensus, Box::new(ConsensusStrategy { count: 2 }));
        let executor = DefaultExecutor::new(provider, strategies);

        let mut node = make_llm_node(StrategyKind::Consensus);
        node.config.insert(
            "messages".into(),
            serde_json::json!([{"role": "user", "content": "original prompt"}]),
        );

        let result = executor.execute_node(&node).await;
        assert_eq!(result.state, NodeState::Succeeded);

        let requests = captured.lock().unwrap();
        // 2 members + 1 judge
        assert!(requests.len() >= 2, "expected at least member + judge calls");

        // The judge request (last one, since judge runs last in topo order)
        let judge_request = requests.last().expect("should have judge request");
        let judge_messages = judge_request
            .messages
            .iter()
            .map(|m| (&m.role, m.content.as_str()))
            .collect::<Vec<_>>();

        // Judge should have a system prompt mentioning judging
        let has_judge_system = judge_messages
            .iter()
            .any(|(r, c)| *r == "system" && c.contains("judge"));
        assert!(has_judge_system, "judge request should have system prompt");

        // CRITICAL: judge should see member outputs in its context
        // (currently broken — judge sees only the original prompt)
        let judge_user_content = judge_messages
            .iter()
            .filter(|(r, _)| *r == "user")
            .map(|(_, c)| *c)
            .collect::<Vec<_>>();
        let sees_member_outputs = judge_user_content.iter().any(|c| c.contains("response-"));
        assert!(
            sees_member_outputs,
            "judge must see member outputs, not just the original prompt. Judge user messages: {:?}",
            judge_user_content
        );
    }

    /// Provider with configurable content and provider-native tool_calls.
    struct ToolCallProvider {
        content: String,
        tool_calls: Option<Vec<ToolCall>>,
    }

    /// Provider that emits tool calls for the first `tool_call_requests`
    /// requests, then falls back to plain text — used to verify the bounded
    /// ReAct-style tool loop in the executor.
    struct ToolLoopProvider {
        text: String,
        tool_call_requests: usize,
        tool_calls: Vec<ToolCall>,
        request_count: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl ChatProvider for ToolLoopProvider {
        fn name(&self) -> &str {
            "tool-loop-provider"
        }

        async fn chat_completion(
            &self,
            _request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            let n = self.request_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let calls = if n < self.tool_call_requests {
                Some(self.tool_calls.clone())
            } else {
                None
            };
            Ok(ChatCompletionResponse {
                id: format!("tool-loop-{}", n),
                object: "chat.completion".into(),
                created: 0,
                model: "tool-loop-model".into(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: self.text.clone(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: calls,
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
            })
        }
    }

    #[async_trait]
    impl ChatProvider for ToolCallProvider {
        fn name(&self) -> &str {
            "tool-call-provider"
        }

        async fn chat_completion(
            &self,
            _request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                id: "tool".into(),
                object: "chat.completion".into(),
                created: 0,
                model: "tool-model".into(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: self.content.clone(),
                    },
                    finish_reason: "stop".into(),
                }],
                usage: None,
                native_tool_calls: self.tool_calls.clone(),
            })
        }
    }

    fn calculator_registry() -> Arc<ToolRegistry> {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(crate::tools::builtin::CalculatorTool));
        registry.register(Arc::new(crate::tools::builtin::SearchTool));
        Arc::new(registry)
    }

    fn single_strategies() -> HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> {
        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(StrategyKind::Single, Box::new(SingleStrategy));
        strategies
    }

    /// Law 7 / ADR-037: a model output containing a free-form tool JSON
    /// object is returned as TEXT and never executed.
    #[tokio::test]
    async fn law7_no_freeform_tool_parsing() {
        let tool_json = r#"{"tool": "calculator", "args": {"expression": "2+2"}}"#;
        let provider = Arc::new(ToolCallProvider {
            content: tool_json.to_string(),
            tool_calls: None,
        });
        let executor = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(calculator_registry())
            .with_allow_auto_exec(true);
        let mut node = make_llm_node(StrategyKind::Single);
        node.config.insert(
            "tool_allowlist".into(),
            serde_json::json!(["calculator"]),
        );

        let result = executor.execute_node(&node).await;

        assert_eq!(result.state, NodeState::Succeeded);
        let output = result.output.expect("output must be present");
        assert_eq!(
            output,
            serde_json::Value::String(tool_json.to_string()),
            "tool-shaped JSON in content must be returned as text, never executed"
        );
        assert!(
            !output.to_string().contains("\"result\""),
            "the calculator must never have run"
        );
    }

    /// Law 7: provider-native tool_calls execute ONLY allowlisted tools.
    #[tokio::test]
    async fn law7_native_tool_calls_execute_only_allowlisted() {
        let provider = Arc::new(ToolCallProvider {
            content: String::new(),
            tool_calls: Some(vec![
                ToolCall {
                    id: "c1".into(),
                    name: "calculator".into(),
                    arguments: serde_json::json!({"expression": "2+2"}),
                },
                ToolCall {
                    id: "s1".into(),
                    name: "search".into(),
                    arguments: serde_json::json!({"query": "x"}),
                },
            ]),
        });
        let executor = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(calculator_registry())
            .with_allow_auto_exec(true);
        let mut node = make_llm_node(StrategyKind::Single);
        node.config.insert(
            "tool_allowlist".into(),
            serde_json::json!(["calculator"]),
        );

        let result = executor.execute_node(&node).await;

        assert_eq!(result.state, NodeState::Succeeded);
        let output = result.output.expect("tool call results must be produced");
        let calls = output["tool_calls"].as_array().expect("tool_calls array");
        assert_eq!(calls.len(), 2);
        let calc = &calls[0];
        assert_eq!(calc["tool"], "calculator");
        assert_eq!(calc["executed"], true);
        assert_eq!(calc["result"]["result"], 4.0, "calculator must run 2+2");
        let search = &calls[1];
        assert_eq!(search["tool"], "search");
        assert_eq!(search["executed"], false, "search is outside the allowlist");
        assert!(
            search["reason"].as_str().unwrap_or("").contains("allowlist"),
            "non-allowlisted call must explain why it was not executed"
        );
    }

    /// Law 7: with auto-execution disabled (default), native tool_calls are
    /// never executed even when the request names an allowlist.
    #[tokio::test]
    async fn law7_native_tool_calls_not_executed_when_auto_exec_disabled() {
        let provider = Arc::new(ToolCallProvider {
            content: String::new(),
            tool_calls: Some(vec![ToolCall {
                id: "c1".into(),
                name: "calculator".into(),
                arguments: serde_json::json!({"expression": "2+2"}),
            }]),
        });
        let executor = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(calculator_registry());
        let mut node = make_llm_node(StrategyKind::Single);
        node.config.insert(
            "tool_allowlist".into(),
            serde_json::json!(["calculator"]),
        );

        let result = executor.execute_node(&node).await;

        assert_eq!(result.state, NodeState::Succeeded);
        let output = result.output.expect("tool call results must be produced");
        assert_eq!(
            output["tool_calls"][0]["executed"],
            false,
            "auto-exec disabled must never execute a tool"
        );
        assert!(
            !output.to_string().contains("\"result\""),
            "the calculator must never have run"
        );
    }

    /// Law 7: an empty per-request allowlist blocks all tool execution
    /// (fail closed), even with auto-exec enabled.
    #[tokio::test]
    async fn law7_empty_allowlist_blocks_all_tools() {
        let provider = Arc::new(ToolCallProvider {
            content: String::new(),
            tool_calls: Some(vec![ToolCall {
                id: "c1".into(),
                name: "calculator".into(),
                arguments: serde_json::json!({"expression": "2+2"}),
            }]),
        });
        let executor = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(calculator_registry())
            .with_allow_auto_exec(true);

        let node = make_llm_node(StrategyKind::Single);

        let result = executor.execute_node(&node).await;
        assert_eq!(result.state, NodeState::Succeeded);
        let output = result.output.expect("tool call results must be produced");
        assert_eq!(
            output["tool_calls"][0]["executed"],
            false,
            "absent allowlist must block all tool execution"
        );
    }

    /// Law 7: when auto-exec is enabled with an allowlist, tool definitions
    /// are advertised to the provider; otherwise the request carries none.
    #[tokio::test]
    async fn law7_tool_definitions_only_sent_with_allowlist() {
        let captured = Arc::new(std::sync::Mutex::new(None::<ChatCompletionRequest>));
        let provider = Arc::new(CapturingMockProvider(captured.clone()));
        let executor = DefaultExecutor::new(provider.clone(), single_strategies())
            .with_tool_registry(calculator_registry())
            .with_allow_auto_exec(true);
        let node = make_llm_node(StrategyKind::Single);

        let _ = executor.execute_node(&node).await;
        let request = captured.lock().unwrap().take().unwrap();
        assert!(
            request.tools.is_none(),
            "no allowlist in request means no tool definitions may be advertised"
        );

        let mut node = make_llm_node(StrategyKind::Single);
        node.config.insert(
            "tool_allowlist".into(),
            serde_json::json!(["calculator"]),
        );
        let _ = executor.execute_node(&node).await;
        let request = captured.lock().unwrap().take().unwrap();
        let tools = request.tools.expect("allowlist must advertise tool definitions");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "calculator");
        assert!(tools[0].parameters.is_some(), "schema must be advertised");

        let executor_disabled = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(calculator_registry());
        let mut node = make_llm_node(StrategyKind::Single);
        node.config.insert(
            "tool_allowlist".into(),
            serde_json::json!(["calculator"]),
        );
        let _ = executor_disabled.execute_node(&node).await;
        let request = captured.lock().unwrap().take().unwrap();
        assert!(
            request.tools.is_none(),
            "auto-exec disabled must not advertise tool definitions"
        );
    }

    /// The bounded tool loop re-prompts the model after executing native tool
    /// calls, then final output is the model's text once it stops calling
    /// tools.
    #[tokio::test]
    async fn test_tool_loop_re_prompts_until_model_emits_text() {
        let provider = Arc::new(ToolLoopProvider {
            text: "final review text".into(),
            tool_call_requests: 2,
            tool_calls: vec![ToolCall {
                id: "lr".into(),
                name: "file_read".into(),
                arguments: serde_json::json!({"path": "src/executor/mod.rs"}),
            }],
            request_count: std::sync::atomic::AtomicUsize::new(0),
        });
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(crate::tools::builtin::FileReadTool::new(".".into())));
        let executor = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(Arc::new(registry))
            .with_allow_auto_exec(true);
        let mut node = make_llm_node(StrategyKind::Single);
        node.config.insert(
            "tool_allowlist".into(),
            serde_json::json!(["file_read"]),
        );

        let result = executor.execute_node(&node).await;

        assert_eq!(result.state, NodeState::Succeeded);
        assert_eq!(
            result.output,
            Some(serde_json::Value::String("final review text".into())),
            "after tool rounds the model's final text must be the output"
        );
        let usage = result.usage.expect("usage accumulated across rounds");
        assert_eq!(usage.total_tokens, 45, "3 provider calls (2 tool + 1 text) x 15 tokens");
    }

    /// The tool loop must terminate even when the model never stops calling
    /// tools — the round budget caps it.
    #[tokio::test]
    async fn test_tool_loop_honors_round_budget() {
        let provider = Arc::new(ToolLoopProvider {
            text: String::new(),
            tool_call_requests: usize::MAX,
            tool_calls: vec![ToolCall {
                id: "c".into(),
                name: "calculator".into(),
                arguments: serde_json::json!({"expression": "1+1"}),
            }],
            request_count: std::sync::atomic::AtomicUsize::new(0),
        });
        let executor = DefaultExecutor::new(provider, single_strategies())
            .with_tool_registry(calculator_registry())
            .with_allow_auto_exec(true);
        let mut node = make_llm_node(StrategyKind::Single);
        node.config.insert(
            "tool_allowlist".into(),
            serde_json::json!(["calculator"]),
        );
        node.config.insert("max_tool_rounds".into(), serde_json::json!(3));

        let result = executor.execute_node(&node).await;

        assert_eq!(result.state, NodeState::Succeeded);
        assert!(
            result.output.is_some(),
            "budget-exhausted loop must still surface the last tool results"
        );
        assert_eq!(result.output.unwrap()["tool_calls"][0]["executed"], true);
    }

    #[cfg(feature = "semantic-cache")]
    mod cache_tests {
        use super::*;
        use crate::cache::embeddings::Embedder;
        use crate::cache::SemanticCache;

        /// Deterministic per-text embeddings: identical keys embed identically
        /// (cosine 1.0), different keys embed differently.
        struct DeterministicEmbedder;

        #[async_trait]
        impl Embedder for DeterministicEmbedder {
            async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
                let mut v = vec![0.0f32; 64];
                for (i, b) in text.bytes().enumerate() {
                    v[i % 64] += b as f32;
                }
                let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                if norm > 0.0 {
                    for x in &mut v {
                        *x /= norm;
                    }
                }
                Ok(v)
            }
        }

        struct CountingProvider(Arc<std::sync::atomic::AtomicUsize>);

        #[async_trait]
        impl ChatProvider for CountingProvider {
            async fn chat_completion(
                &self,
                request: &ChatCompletionRequest,
            ) -> anyhow::Result<ChatCompletionResponse> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(ChatCompletionResponse {
                    id: "count".into(),
                    object: "chat.completion".into(),
                    created: 0,
                    model: request.model.clone(),
                    choices: vec![Choice {
                        index: 0,
                        message: ChatMessage {
                            role: "assistant".into(),
                            content: "judge verdict".into(),
                        },
                        finish_reason: "stop".into(),
                    }],
                    native_tool_calls: None,
                    usage: Some(Usage {
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        total_tokens: 2,
                    }),
                })
            }

            fn name(&self) -> &str {
                "counting"
            }
        }

        /// A cache hit on one consensus member must satisfy only that member:
        /// the remaining members and the judge still execute, and the judge's
        /// output becomes the strategy result (regression for the early
        /// `return` that used to abort the whole subgraph).
        #[tokio::test]
        async fn test_cache_hit_continues_remaining_subgraph() {
            let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let provider = Arc::new(CountingProvider(counter.clone()));
            let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> =
                HashMap::new();
            strategies.insert(
                StrategyKind::Consensus,
                Box::new(ConsensusStrategy::default()),
            );
            let executor = DefaultExecutor::new(provider, strategies);

            let mut node = make_llm_node(StrategyKind::Consensus);
            node.config.insert(
                "messages".into(),
                serde_json::json!([{ "role": "user", "content": "hello cache test" }]),
            );

            // Reconstruct the exact request the first member will produce and
            // pre-populate the cache for it.
            let member_request = ChatCompletionRequest {
                model: node.model.clone(),
                messages: vec![ChatMessage {
                    role: "user".into(),
                    content: "hello cache test".into(),
                }],
                stream: false,
                temperature: None,
                max_tokens: None,
                tools: None,
                files: None,
                execution: None,
                output: None,
                strategy: None,
            };
            let cache_key = DefaultExecutor::cache_key(&member_request);

            let cache = Arc::new(SemanticCache::new(
                Arc::new(DeterministicEmbedder),
                0.99,
                100,
                64,
            ));
            cache
                .put(&cache_key, serde_json::json!({ "content": "cached member" }))
                .await;
            let executor = executor.with_cache(cache);

            let result = executor.execute_node(&node).await;

            assert_eq!(result.state, NodeState::Succeeded);
            assert_eq!(
                result.output,
                Some(serde_json::Value::String("judge verdict".into())),
                "a cached member must not become the whole strategy's output"
            );
            assert_eq!(
                counter.load(std::sync::atomic::Ordering::SeqCst),
                1,
                "only the judge should hit the provider (all members cached)"
            );
        }
    }

    #[tokio::test]
    async fn test_execute_node_preserves_accumulated_usage_on_failure() {
        struct FailingProvider {
            calls: std::sync::atomic::AtomicUsize,
        }

        #[async_trait]
        impl ChatProvider for FailingProvider {
            fn name(&self) -> &str { "failing" }
            async fn chat_completion(&self, _req: &ChatCompletionRequest) -> anyhow::Result<ChatCompletionResponse> {
                let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n == 0 {
                    Ok(ChatCompletionResponse {
                        id: "ok".into(),
                        object: "chat.completion".into(),
                        created: 0,
                        model: "mock".into(),
                        choices: vec![Choice { index: 0, message: ChatMessage { role: "assistant".into(), content: "ok".into() }, finish_reason: "stop".into() }],
                        native_tool_calls: None,
                        usage: Some(Usage { prompt_tokens: 10, completion_tokens: 20, total_tokens: 30 }),
                    })
                } else {
                    anyhow::bail!("provider simulated failure")
                }
            }
        }

        let provider = Arc::new(FailingProvider { calls: std::sync::atomic::AtomicUsize::new(0) });
        let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
        strategies.insert(StrategyKind::Consensus, Box::new(crate::strategies::consensus::ConsensusStrategy::default()));
        let executor = DefaultExecutor::new(provider, strategies);
        let node = make_llm_node(StrategyKind::Consensus);

        let result = executor.execute_node(&node).await;

        assert!(matches!(result.state, NodeState::Failed(_)));
        let usage = result.usage.expect("accumulated usage from successful first stage must be preserved on second stage failure");
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }
}
