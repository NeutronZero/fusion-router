use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use tracing::info;

#[cfg(feature = "semantic-cache")]
use crate::cache::SemanticCache;
use crate::executor::Executor;
use crate::providers::ChatProvider;
use crate::strategies::Strategy;
use crate::tools::ToolRegistry;
use crate::types::{
    ChatCompletionRequest, ChatMessage, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph,
    NodeExecutionResult, NodeState, StrategyKind, Usage,
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
    pub(crate) fn cache_key(request: &ChatCompletionRequest) -> String {
        let messages_json = serde_json::to_string(&request.messages).unwrap_or_default();
        format!("{}:{}", request.model, messages_json)
    }
}

#[async_trait]
impl Executor for DefaultExecutor {
    #[tracing::instrument(skip(self, node), fields(node_id = %node.id, model = %node.model, kind = ?node.kind, strategy = ?node.strategy))]
    async fn execute_node(&self, node: &ExecutionNode) -> NodeExecutionResult {
        let start = std::time::Instant::now();
        let strategy_label = node.strategy.as_label();
        let subgraph = self.resolve_strategy(node).await;
        let mut accumulated_usage: Option<Usage> = None;
        let mut output_value: Option<serde_json::Value> = None;

        tracing::debug!(
            strategy = %strategy_label,
            subgraph_nodes = subgraph.nodes.len(),
            "strategy execution started"
        );

        let n_sub_nodes = subgraph.nodes.len();
        let mut incoming: HashMap<uuid::Uuid, Vec<uuid::Uuid>> = HashMap::with_capacity(n_sub_nodes);
        for edge in &subgraph.edges {
            incoming.entry(edge.to).or_default().push(edge.from);
        }
        let mut remaining: Vec<&ExecutionNode> = subgraph.nodes.iter().collect();
        let mut completed: std::collections::HashSet<uuid::Uuid> = std::collections::HashSet::with_capacity(n_sub_nodes);
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

        let mut node_outputs: HashMap<uuid::Uuid, serde_json::Value> = HashMap::with_capacity(n_sub_nodes);

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

                        if let Some(usage) = response.usage {
                            accumulated_usage = Some(match accumulated_usage {
                                Some(acc) => Usage {
                                    prompt_tokens: acc.prompt_tokens + usage.prompt_tokens,
                                    completion_tokens: acc.completion_tokens + usage.completion_tokens,
                                    total_tokens: acc.total_tokens + usage.total_tokens,
                                },
                                None => usage,
                            });
                        }

                        let native_calls = response.native_tool_calls.as_deref().unwrap_or(&[]);
                        let results = self
                            .execute_native_tool_calls(sub_node, native_calls)
                            .await;
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
                            content: response.choices.first()
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
                    }; // tool loop

                    #[cfg(feature = "semantic-cache")]
                    if let Some(ref cache) = self.cache {
                        if let Some(content) = output_value.as_ref().and_then(|v| v.as_str()) {
                            if !content.trim().is_empty() {
                                cache.put(&cache_key, serde_json::json!({ "content": content })).await;
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

    async fn resolve_strategy(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        self.resolve_strategy_impl(node).await
    }
}
