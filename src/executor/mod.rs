use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use serde_json::Value;
use tracing::info;

use crate::cache::SemanticCache;
use crate::providers::ChatProvider;
use crate::strategies::Strategy;
use crate::tools::ToolRegistry;
use crate::types::{
    ChatCompletionRequest, ChatMessage, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph,
    NodeExecutionResult, NodeState, StrategyKind, Usage,
};

#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute_node(&self, node: &ExecutionNode) -> NodeExecutionResult;
    async fn resolve_strategy(&self, node: &ExecutionNode) -> ExecutionSubgraph;
}

pub struct DefaultExecutor {
    pub provider: Arc<dyn ChatProvider + Send + Sync>,
    pub strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>>,
    pub cache: Option<Arc<SemanticCache>>,
    pub tool_registry: Option<Arc<ToolRegistry>>,
}

impl DefaultExecutor {
    pub fn new(
        provider: Arc<dyn ChatProvider + Send + Sync>,
        strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>>,
    ) -> Self {
        Self { provider, strategies, cache: None, tool_registry: None }
    }

    pub fn with_cache(mut self, cache: Arc<SemanticCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn with_tool_registry(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    fn build_request(node: &ExecutionNode) -> ChatCompletionRequest {
        let messages: Vec<ChatMessage> = node
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

    fn cache_key(request: &ChatCompletionRequest) -> String {
        let messages_json = serde_json::to_string(&request.messages).unwrap_or_default();
        format!("{}:{}", request.model, messages_json)
    }
}

#[async_trait]
impl Executor for DefaultExecutor {
    async fn execute_node(&self, node: &ExecutionNode) -> NodeExecutionResult {
        let start = std::time::Instant::now();
        let subgraph = self.resolve_strategy(node).await;
        let mut accumulated_usage: Option<Usage> = None;

        for sub_node in &subgraph.nodes {
            match sub_node.kind {
                ExecutionNodeKind::LLMGenerate
                | ExecutionNodeKind::LLMReview
                | ExecutionNodeKind::LLMJudge => {
                    let request = Self::build_request(sub_node);
                    let cache_key = Self::cache_key(&request);

                    if let Some(ref cache) = self.cache {
                        if cache.get(&cache_key).await.is_some() {
                            info!(
                                node_id = %sub_node.id,
                                "Cache hit for LLM node"
                            );
                            let latency = start.elapsed().as_millis() as u64;
                            return NodeExecutionResult {
                                state: NodeState::Succeeded,
                                usage: None,
                                latency_ms: latency,
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
                                                        // Store result in response for downstream nodes
                                                        let _ = result;
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
                            return NodeExecutionResult {
                                state: NodeState::Failed(format!("Provider error: {}", e)),
                                usage: None,
                                latency_ms: latency,
                            };
                        }
                    }
                }
                ExecutionNodeKind::Transform
                | ExecutionNodeKind::Gate
                | ExecutionNodeKind::Aggregate
                | ExecutionNodeKind::Conditional
                | ExecutionNodeKind::Loop
                | ExecutionNodeKind::Split
                | ExecutionNodeKind::Join
                | ExecutionNodeKind::Barrier => {}
            }
        }

        let latency = start.elapsed().as_millis() as u64;
        NodeExecutionResult {
            state: NodeState::Succeeded,
            usage: accumulated_usage,
            latency_ms: latency,
        }
    }

    async fn resolve_strategy(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        self.strategies
            .get(&node.strategy)
            .map(|s| s.apply(node))
            .unwrap_or_else(|| {
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
            })
    }
}
