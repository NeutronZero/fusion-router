use async_trait::async_trait;
use fusion_scheduler::Executor as _;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

#[cfg(feature = "semantic-cache")]
use crate::cache::SemanticCache;
use crate::executor::fusion_bridge::{FusionChatProvider, CANCELLED_MARKER};
use crate::executor::Executor;
use crate::providers::ChatProvider;
use crate::strategies::Strategy;
use crate::tools::ToolRegistry;
use crate::transport::backoff::Backoff;
use crate::types::{
    ChatCompletionRequest, ChatMessage, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph,
    NodeExecContext, NodeExecutionResult, NodeState, StrategyKind, Usage,
};

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

    #[tracing::instrument(skip_all, fields(node_id = %node.id, model = %node.model))]
    pub(crate) fn build_request(&self, node: &ExecutionNode) -> ChatCompletionRequest {
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

        if !messages.iter().any(|m| m.role == "system") {
            let system_prompt = match node.kind {
                ExecutionNodeKind::LLMJudge => Some("You are a judge evaluating the quality and correctness of responses. Assess the provided answers critically and select the best one, explaining your reasoning."),
                _ => match node.strategy {
                    crate::types::StrategyKind::Reflection => Some("You are a reflective reviewer. Analyze the previous response, identify potential issues, and provide an improved version."),
                    _ => None,
                },
            };
            if let Some(prompt) = system_prompt {
                messages.insert(
                    0,
                    ChatMessage {
                        role: "system".to_string(),
                        content: prompt.to_string(),
                    },
                );
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
    pub(crate) fn cache_key(request: &ChatCompletionRequest) -> String {
        let messages_json = serde_json::to_string(&request.messages).unwrap_or_default();
        format!("{}:{}", request.model, messages_json)
    }
}

impl DefaultExecutor {
    /// Phase 6.4 delegation boundary. Plain single-model LLM leaves (no
    /// strategy, no compile-time subgraph, no tool allowlist) execute through
    /// `fusion_runtime::ProviderExecutor` — which owns retry/fallback,
    /// budget gating and the Law 7 tool loop for the delegated path.
    ///
    /// Everything else (strategy nodes — runtime-lowered or prebuilt — and any
    /// node with a tool allowlist) stays on the legacy src executor path,
    /// which owns the Law 7 tool-call machinery and runtime strategy
    /// expansion until crates parity exists for those (6.6 debt).
    pub(crate) fn delegate_to_crates(&self, node: &ExecutionNode) -> bool {
        node.subgraph.is_none()
            && node.strategy == StrategyKind::Single
            && !node.config.contains_key("tool_allowlist")
    }

    /// Law 7 boundary: the crates executor refuses (fails) when the provider
    /// returns native `tool_calls` and auto-exec is disabled — it owns no
    /// src-style tool semantics. The src contract surfaces those calls as
    /// `executed: false` metadata and succeeds, so tool-refusal failures are
    /// re-routed to the legacy path (which never auto-executes by default).
    fn is_tool_refusal(reason: &str) -> bool {
        reason.contains("auto-exec is disabled")
            || reason.contains("not allowed for node")
            || reason.contains("not registered (node")
            || reason.contains("exceeded max_tool_iterations")
    }

    /// Runs a plain leaf node on `fusion_runtime::ProviderExecutor`.
    ///
    /// Retries/fallback happen inside the crates executor
    /// (`retry_policy` + `node.fallback`); the scheduler never sees them.
    /// Outputs are mapped back to the src contract: LLM text becomes a
    /// `String`, control-flow markers become `None` (matching the legacy
    /// path's observable behavior).
    async fn execute_crates(
        &self,
        node: &ExecutionNode,
        ctx: &NodeExecContext,
    ) -> NodeExecutionResult {
        #[cfg(feature = "semantic-cache")]
        let provider: Arc<dyn fusion_runtime::ChatProvider> =
            Arc::new(FusionChatProvider::new(self.provider.clone()).with_cache(self.cache.clone()));
        #[cfg(not(feature = "semantic-cache"))]
        let provider: Arc<dyn fusion_runtime::ChatProvider> =
            Arc::new(FusionChatProvider::new(self.provider.clone()));
        let crates_executor = fusion_runtime::ProviderExecutor::new(provider);
        let mut result = crates_executor.execute_node(node, ctx).await;
        if let NodeState::Failed(reason) = &result.state {
            if Self::is_tool_refusal(reason) {
                return self.execute_legacy(node).await;
            }
        }
        match node.kind {
            ExecutionNodeKind::LLMGenerate
            | ExecutionNodeKind::LLMReview
            | ExecutionNodeKind::LLMJudge => {
                if let Some(out) = result.output.as_ref() {
                    if let Some(content) = out.get("content").and_then(|v| v.as_str()) {
                        result.output = Some(serde_json::Value::String(content.to_string()));
                    }
                }
            }
            _ => {
                result.output = None;
            }
        }
        result
    }

    /// Legacy strategy path with the retry/fallback loop the monolith
    /// scheduler used to own (moved in from `CratesExecutorAdapter` in 6.4,
    /// so the adapter is a pure forwarder). Never retries a
    /// `"Cancelled by client"` failure: the crates loop already raced the
    /// cancellation token per node.
    async fn execute_legacy_with_retry(&self, node: &ExecutionNode) -> NodeExecutionResult {
        let max_retries = node.retry_policy.max_retries;
        let mut attempts: u32 = 0;
        let mut backoff: Option<Backoff> = None;
        // Usage from successful stages accumulates across attempts: a retry
        // that fails on an earlier stage must not lose the tokens spent by
        // the stage that already succeeded.
        let mut total_usage: Option<Usage> = None;

        fn merge_usage(acc: &mut Option<Usage>, usage: Option<&Usage>) {
            if let Some(u) = usage {
                *acc = Some(match acc.take() {
                    Some(prev) => Usage {
                        prompt_tokens: prev.prompt_tokens + u.prompt_tokens,
                        completion_tokens: prev.completion_tokens + u.completion_tokens,
                        total_tokens: prev.total_tokens + u.total_tokens,
                    },
                    None => u.clone(),
                });
            }
        }

        loop {
            let result = self.execute_legacy(node).await;
            merge_usage(&mut total_usage, result.usage.as_ref());
            match &result.state {
                NodeState::Succeeded => {
                    let mut result = result;
                    result.usage = total_usage;
                    return result;
                }
                NodeState::Failed(reason) if reason.as_str() == CANCELLED_MARKER => return result,
                NodeState::Failed(_) if attempts < max_retries => {
                    attempts += 1;
                    info!(
                        node_id = ?node.id,
                        attempt = attempts,
                        max = max_retries,
                        "Retrying node after backoff"
                    );
                    if node.retry_policy.backoff_ms > 0 {
                        let max_ms = node.retry_policy.backoff_ms.saturating_mul(10);
                        let b = backoff.get_or_insert_with(|| {
                            Backoff::new(node.retry_policy.backoff_ms, max_ms)
                        });
                        tokio::time::sleep(b.next()).await;
                    }
                    continue;
                }
                NodeState::Failed(_) => {
                    if let Some(ref fallback) = node.fallback {
                        info!(
                            node_id = ?node.id,
                            fallback_model = %fallback.model,
                            "Attempting fallback execution"
                        );
                        let mut fallback_node = node.clone();
                        fallback_node.model = fallback.model.clone();
                        let fb_result = self.execute_legacy(&fallback_node).await;
                        merge_usage(&mut total_usage, fb_result.usage.as_ref());
                        return match fb_result.state {
                            NodeState::Succeeded => {
                                let mut fb_result = fb_result;
                                fb_result.usage = total_usage;
                                fb_result
                            }
                            NodeState::Failed(fb_reason) => NodeExecutionResult {
                                state: NodeState::Failed(format!("Fallback failed: {}", fb_reason)),
                                usage: total_usage,
                                latency_ms: fb_result.latency_ms,
                                output: None,
                            },
                            other => NodeExecutionResult {
                                state: other,
                                usage: total_usage,
                                latency_ms: fb_result.latency_ms,
                                output: fb_result.output,
                            },
                        };
                    }
                    let mut result = result;
                    result.usage = total_usage;
                    return result;
                }
                _ => {
                    let mut result = result;
                    result.usage = total_usage;
                    return result;
                }
            }
        }
    }

    /// The strategy execution body: use the prebuilt subgraph (compiled at
    /// compile time by `fusion_compiler::strategy_expansion`), run members
    /// topologically with the Law 7 tool loop, accumulate usage, surface the
    /// exit output.
    #[tracing::instrument(skip(self, node), fields(node_id = %node.id, model = %node.model, kind = ?node.kind, strategy = ?node.strategy))]
    async fn execute_legacy(&self, node: &ExecutionNode) -> NodeExecutionResult {
        let start = std::time::Instant::now();
        let strategy_label = node.strategy.as_label();
        let subgraph = match &node.subgraph {
            Some(sg) => sg.clone(),
            None => {
                // Phase D: expand on-the-fly if no prebuilt subgraph exists.
                // Phase C guarantees this never happens in production.
                fusion_compiler::strategy_expansion::expanded_subgraph(node).unwrap_or_else(|| {
                    crate::types::ExecutionSubgraph {
                        nodes: vec![node.clone()],
                        edges: vec![],
                        entry_node_id: node.id,
                        exit_node_id: node.id,
                    }
                })
            }
        };
        let mut subgraph = subgraph;
        propagate_parent_messages(node, &mut subgraph);
        let mut accumulated_usage: Option<Usage> = None;
        let mut output_value: Option<serde_json::Value> = None;

        tracing::debug!(
            strategy = %strategy_label,
            subgraph_nodes = subgraph.nodes.len(),
            "strategy execution started"
        );

        let n_sub_nodes = subgraph.nodes.len();
        let mut incoming: HashMap<uuid::Uuid, Vec<uuid::Uuid>> =
            HashMap::with_capacity(n_sub_nodes);
        for edge in &subgraph.edges {
            incoming.entry(edge.to).or_default().push(edge.from);
        }
        let mut remaining: Vec<&ExecutionNode> = subgraph.nodes.iter().collect();
        let mut completed: std::collections::HashSet<uuid::Uuid> =
            std::collections::HashSet::with_capacity(n_sub_nodes);
        let mut order: Vec<&ExecutionNode> = Vec::with_capacity(n_sub_nodes);
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

        let mut node_outputs: HashMap<uuid::Uuid, serde_json::Value> =
            HashMap::with_capacity(n_sub_nodes);

        for sub_node in order {
            match sub_node.kind {
                ExecutionNodeKind::LLMGenerate
                | ExecutionNodeKind::LLMReview
                | ExecutionNodeKind::LLMJudge => {
                    let mut request = self.build_request(sub_node);
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
                            output_value = Some(cached_output);
                            if let Some(current) = output_value.clone() {
                                node_outputs.insert(sub_node.id, current);
                            }
                            continue;
                        }
                    }

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
                                // Cancellation markers pass through verbatim so the
                                // retry wrap never re-queues a cancelled node.
                                let reason = if e.to_string() == CANCELLED_MARKER {
                                    CANCELLED_MARKER.to_string()
                                } else {
                                    format!("Provider error: {}", e)
                                };
                                return NodeExecutionResult {
                                    state: NodeState::Failed(reason),
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

                        output_value = response
                            .choices
                            .first()
                            .map(|c| c.message.content.clone())
                            .map(serde_json::Value::String);

                        if let Some(usage) = response.usage {
                            accumulated_usage = Some(match accumulated_usage {
                                Some(acc) => Usage {
                                    prompt_tokens: acc.prompt_tokens + usage.prompt_tokens,
                                    completion_tokens: acc.completion_tokens
                                        + usage.completion_tokens,
                                    total_tokens: acc.total_tokens + usage.total_tokens,
                                },
                                None => usage,
                            });
                        }

                        let native_calls = response.native_tool_calls.as_deref().unwrap_or(&[]);
                        let results = self.execute_native_tool_calls(sub_node, native_calls).await;
                        let executed_any = results["tool_calls"]
                            .as_array()
                            .map(|c| c.iter().any(|x| x["executed"] == true))
                            .unwrap_or(false);

                        if !executed_any || tool_round + 1 >= max_tool_rounds {
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

                        request.messages.push(ChatMessage {
                            role: "assistant".to_string(),
                            content: response
                                .choices
                                .first()
                                .map(|c| c.message.content.clone())
                                .unwrap_or_default(),
                        });
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
                    } // tool loop

                    #[cfg(feature = "semantic-cache")]
                    if let Some(ref cache) = self.cache {
                        if let Some(content) = output_value.as_ref().and_then(|v| v.as_str()) {
                            if !content.trim().is_empty() {
                                cache
                                    .put(&cache_key, serde_json::json!({ "content": content }))
                                    .await;
                            }
                        }
                    }

                    if let Some(ref current) = output_value {
                        node_outputs.insert(sub_node.id, current.clone());
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
}

#[async_trait]
impl Executor for DefaultExecutor {
    #[tracing::instrument(skip(self, node, ctx), fields(node_id = %node.id, model = %node.model, kind = ?node.kind, strategy = ?node.strategy))]
    async fn execute_node(
        &self,
        node: &ExecutionNode,
        ctx: &NodeExecContext,
    ) -> NodeExecutionResult {
        if self.delegate_to_crates(node) {
            self.execute_crates(node, ctx).await
        } else {
            self.execute_legacy_with_retry(node).await
        }
    }
}

/// Copies the parent node's messages and tool allowlist into any LLM sub-node
/// that lacks them, ensuring requests never go out with empty `messages` arrays.
pub(crate) fn propagate_parent_messages(node: &ExecutionNode, subgraph: &mut ExecutionSubgraph) {
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
        if !matches!(
            sub_node.kind,
            ExecutionNodeKind::LLMGenerate
                | ExecutionNodeKind::LLMReview
                | ExecutionNodeKind::LLMJudge
        ) {
            continue;
        }
        let has_messages = sub_node
            .config
            .get("messages")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        if !has_messages {
            sub_node
                .config
                .insert("messages".to_string(), messages.clone());
        }
        if sub_node.config.get("tool_allowlist").is_none() {
            if let Some(ref allowlist) = tool_allowlist {
                sub_node
                    .config
                    .insert("tool_allowlist".to_string(), allowlist.clone());
            }
        }
    }
}
