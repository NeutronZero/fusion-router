//! Runtime engine — executes scheduled workflows against providers.
//!
//! Provides:
//! - `ChatProvider` trait for LLM dispatch
//! - `MockProvider` for deterministic testing
//! - `RuntimeEngine` that integrates scheduler + provider

use async_trait::async_trait;
use fusion_scheduler::{DefaultScheduler, Executor, ExecutionOutcome};
use fusion_types::{ExecutionGraph, ExecutionNode, NodeExecContext, NodeExecutionResult, NodeState, Usage};
use std::sync::Arc;

/// Chat completion request sent to a provider.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
}

/// A chat message with role and content.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Response from a chat provider.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub usage: Option<Usage>,
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
        Self { response_prefix: response_prefix.into() }
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
        })
    }
}

/// Provider-backed executor that satisfies the scheduler's `Executor` trait.
/// Routes each node's model string to the chat provider.
struct ProviderExecutor {
    provider: Arc<dyn ChatProvider>,
}

#[async_trait]
impl Executor for ProviderExecutor {
    async fn execute_node(&self, node: &ExecutionNode, ctx: &NodeExecContext) -> NodeExecutionResult {
        let request = ChatRequest {
            model: node.model.clone(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: format!("Execute node {:?}", node.kind),
            }],
        };

        match self.provider.chat_completion(&request).await {
            Ok(response) => NodeExecutionResult {
                state: NodeState::Succeeded,
                usage: response.usage,
                latency_ms: 10,
                output: Some(serde_json::json!({
                    "content": response.content,
                    "node_id": node.id.to_string(),
                })),
            },
            Err(e) => NodeExecutionResult {
                state: NodeState::Failed(e),
                usage: None,
                latency_ms: 0,
                output: None,
            },
        }
    }
}

/// Full runtime engine that integrates the scheduler with a chat provider.
pub struct RuntimeEngine {
    scheduler: DefaultScheduler,
    provider: Arc<dyn ChatProvider>,
}

impl RuntimeEngine {
    pub fn new(provider: Arc<dyn ChatProvider>) -> Self {
        Self {
            scheduler: DefaultScheduler::new(),
            provider,
        }
    }

    pub fn with_max_concurrent(provider: Arc<dyn ChatProvider>, max_concurrent: usize) -> Self {
        Self {
            scheduler: DefaultScheduler::with_max_concurrent(max_concurrent),
            provider,
        }
    }

    /// Execute a full execution graph to completion.
    pub async fn run(&self, graph: Arc<ExecutionGraph>) -> Result<ExecutionOutcome, String> {
        let executor = ProviderExecutor {
            provider: self.provider.clone(),
        };
        self.scheduler.run(graph, &executor).await.map_err(|e| format!("{:?}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_types::*;

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
                    retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                    fallback: None,
                    config: std::collections::HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: n2,
                    kind: ExecutionNodeKind::LLMReview,
                    strategy: StrategyKind::Single,
                    model: "review-model".into(),
                    retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                    fallback: None,
                    config: std::collections::HashMap::new(),
                    subgraph: None,
                },
            ],
            edges: vec![ExecutionEdge { from: n1, to: n2, condition: None }],
            metadata: GraphMetadata {
                estimated_cost: 0.01,
                estimated_tokens: 200,
                max_depth: 2,
                node_count: 2,
            },
            total_tokens: 200,
            total_cost: 10,
            primitive_graph_hash: 0,
        })
    }

    #[tokio::test]
    async fn test_mock_provider_returns_fixed_response() {
        let provider = MockProvider::new("hello");
        let response = provider.chat_completion(&ChatRequest {
            model: "gpt-4".into(),
            messages: vec![],
        }).await.expect("chat");
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
}
