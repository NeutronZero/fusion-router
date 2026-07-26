use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use serde_json::Value;
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
    ExecutionSubgraph, NodeExecutionResult, NodeState, StrategyKind, Usage,
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

    #[tracing::instrument(skip_all, fields(node_id = %node.id, model = %node.model))]
    fn build_request(node: &ExecutionNode) -> ChatCompletionRequest {
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
            tools: None,
            files: None,
            execution: None,
            output: None,
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

        for sub_node in &subgraph.nodes {
            match sub_node.kind {
                ExecutionNodeKind::LLMGenerate
                | ExecutionNodeKind::LLMReview
                | ExecutionNodeKind::LLMJudge => {
                    let request = Self::build_request(sub_node);
                    #[cfg(feature = "semantic-cache")]
                    let cache_key = Self::cache_key(&request);

                    #[cfg(feature = "semantic-cache")]
                    if let Some(ref cache) = self.cache {
                        if let Some(cached) = cache.get(&cache_key).await {
                            info!(
                                node_id = %sub_node.id,
                                "Cache hit for LLM node"
                            );
                            let latency = start.elapsed().as_millis() as u64;
                            let cached_output = cached
                                .get("content")
                                .and_then(|v| v.as_str())
                                .map(|s| serde_json::Value::String(s.to_string()))
                                .unwrap_or(cached);
                            return NodeExecutionResult {
                                state: NodeState::Succeeded,
                                usage: None,
                                latency_ms: latency,
                                output: Some(cached_output),
                            };
                        }
                    }

                    match self.provider.chat_completion(&request).await {
                        Ok(response) => {
                            info!(
                                node_id = %sub_node.id,
                                model = %response.model,
                                "LLM node completed"
                            );

                            output_value = response.choices.first()
                                .map(|c| c.message.content.clone())
                                .map(serde_json::Value::String);

                            #[cfg(feature = "semantic-cache")]
                            if let Some(ref cache) = self.cache {
                                let content = response.choices.first()
                                    .map(|c| c.message.content.clone())
                                    .unwrap_or_default();
                                cache.put(&cache_key, serde_json::json!({ "content": content })).await;
                            }

                            if let Some(ref tool_registry) = self.tool_registry {
                                if let Some(content) = response.choices.first()
                                    .map(|c| c.message.content.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                {
                                    if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&content) {
                                        if let Some(tool_name) = obj.get("tool").and_then(|v| v.as_str()) {
                                            if tool_registry.contains(tool_name) {
                                                let tool = tool_registry.get(tool_name).unwrap();
                                                let tool_args = obj.get("args").cloned().unwrap_or(Value::Null);
                                                match tool.execute(tool_args).await {
                                                    Ok(result) => {
                                                        info!(tool = %tool_name, "Tool executed successfully");
                                                        output_value = Some(serde_json::json!({
                                                            "tool": tool_name,
                                                            "result": result,
                                                        }));
                                                    }
                                                    Err(e) => {
                                                        info!(tool = %tool_name, error = %e, "Tool execution failed");
                                                    }
                                                }
                                            } else {
                                                info!(tool = %tool_name, "Unknown tool requested");
                                            }
                                        }
                                    }
                                }
                            }

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
                        }
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
                                usage: None,
                                latency_ms: latency,
                                output: None,
                            };
                        }
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
        let strategy = self.strategies.get(&node.strategy);
        if let Some(s) = strategy {
            let ctx = CompilationContext::new();
            let ir = strategy_ir_from_node(node);
            if let Ok(pg) = s.lower(&ir, &ctx) {
                let eg = pg.to_execution_graph(
                    node.strategy.clone(),
                    &node.retry_policy,
                    &node.fallback,
                    &node.config,
                );
                return execution_graph_to_subgraph(&eg, node);
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
    match node.strategy {
        StrategyKind::Single => StrategyIR::Single,
        StrategyKind::Consensus => StrategyIR::Consensus {
            count: node.config.get("count").and_then(|v| v.as_u64()).unwrap_or(3) as u32,
        },
        StrategyKind::Debate => StrategyIR::Debate {
            roles: node.config.get("roles")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
        },
        StrategyKind::Reflection => StrategyIR::Reflection {
            max_cycles: node.config.get("max_reflection_cycles")
                .and_then(|v| v.as_u64()).unwrap_or(3) as u32,
        },
        StrategyKind::ReAct => StrategyIR::ReAct {
            max_iterations: node.config.get("max_iterations")
                .and_then(|v| v.as_u64()).unwrap_or(10) as u32,
        },
        StrategyKind::Chain => StrategyIR::Chain {
            stages: node.config.get("stages")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
        },
        StrategyKind::Fusion => StrategyIR::Custom {
            name: "fusion".into(),
            config: serde_json::json!({}),
        },
        StrategyKind::Custom(ref name) => StrategyIR::Custom {
            name: name.clone(),
            config: node.config.get("config").cloned().unwrap_or(serde_json::json!({})),
        },
    }
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
}
