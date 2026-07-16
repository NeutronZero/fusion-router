use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use tracing::info;

use crate::providers::ChatProvider;
use crate::strategies::Strategy;
use crate::types::{
    ChatCompletionRequest, ChatMessage, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph,
    NodeState, StrategyKind,
};

#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute_node(&self, node: &ExecutionNode) -> Result<NodeState, anyhow::Error>;
    async fn resolve_strategy(&self, node: &ExecutionNode) -> ExecutionSubgraph;
}

pub struct DefaultExecutor {
    pub provider: Arc<dyn ChatProvider + Send + Sync>,
    pub strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>>,
}

impl DefaultExecutor {
    pub fn new(
        provider: Arc<dyn ChatProvider + Send + Sync>,
        strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>>,
    ) -> Self {
        Self { provider, strategies }
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
        }
    }
}

#[async_trait]
impl Executor for DefaultExecutor {
    async fn execute_node(&self, node: &ExecutionNode) -> Result<NodeState, anyhow::Error> {
        let subgraph = self.resolve_strategy(node).await;

        for sub_node in &subgraph.nodes {
            match sub_node.kind {
                ExecutionNodeKind::LLMGenerate
                | ExecutionNodeKind::LLMReview
                | ExecutionNodeKind::LLMJudge => {
                    let request = Self::build_request(sub_node);
                    match self.provider.chat_completion(&request).await {
                        Ok(response) => {
                            info!(
                                node_id = %sub_node.id,
                                model = %response.model,
                                "LLM node completed"
                            );
                        }
                        Err(e) => {
                            info!(
                                node_id = %sub_node.id,
                                error = %e,
                                "LLM node failed"
                            );
                            return Ok(NodeState::Failed(format!("Provider error: {}", e)));
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

        Ok(NodeState::Succeeded)
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
