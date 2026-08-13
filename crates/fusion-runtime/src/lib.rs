//! Runtime engine — executes scheduled workflows against providers.
//!
//! Provides:
//! - `ChatProvider` trait for LLM dispatch
//! - `MockProvider` for deterministic testing
//! - `RuntimeEngine` that integrates scheduler + provider

use async_trait::async_trait;
use fusion_scheduler::{DefaultScheduler, Executor, ExecutionOutcome};
use fusion_types::{
    ExecutionGraph, ExecutionNode, ExecutionNodeKind, NodeExecContext, NodeExecutionResult,
    NodeState, StrategyKind, Usage,
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

/// Spy provider that records the last request for assertions.
pub struct SpyProvider {
    pub last_request: std::sync::Mutex<Option<ChatRequest>>,
    response_prefix: String,
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
        })
    }
}

/// Provider-backed executor that satisfies the scheduler's `Executor` trait.
/// Routes each node's model string to the chat provider.
pub struct ProviderExecutor {
    provider: Arc<dyn ChatProvider>,
}

impl ProviderExecutor {
    pub fn new(provider: Arc<dyn ChatProvider>) -> Self {
        Self { provider }
    }

    /// Builds a `ChatRequest` from node config and execution context.
    ///
    /// Order:
    /// 1. `config["messages"]` (JSON array of {role, content}) if present
    /// 2. System prompt injected by kind (Judge) / strategy (Reflection) when
    ///    no system message exists
    /// 3. Parent outputs appended as user messages (Judge / Review / Generate)
    pub fn build_request(&self, node: &ExecutionNode, ctx: &NodeExecContext) -> ChatRequest {
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
                messages.insert(0, ChatMessage {
                    role: "system".into(),
                    content: prompt.to_string(),
                });
            }
        }

        // Append parent outputs as user context
        if !ctx.parent_outputs.is_empty() {
            match node.kind {
                ExecutionNodeKind::LLMJudge
                | ExecutionNodeKind::LLMReview
                | ExecutionNodeKind::LLMGenerate => {
                    for (parent_id, output) in &ctx.parent_outputs {
                        messages.push(ChatMessage {
                            role: "user".into(),
                            content: format!(
                                "Context from parent node {}:\n{}",
                                parent_id, output
                            ),
                        });
                    }
                }
                _ => {}
            }
        }

        ChatRequest {
            model: node.model.clone(),
            messages,
            temperature,
            max_tokens,
        }
    }
}

#[async_trait]
impl Executor for ProviderExecutor {
    async fn execute_node(&self, node: &ExecutionNode, ctx: &NodeExecContext) -> NodeExecutionResult {
        if matches!(
            node.kind,
            ExecutionNodeKind::Gate
                | ExecutionNodeKind::Transform
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

        let request = self.build_request(node, ctx);

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
            temperature: None,
            max_tokens: None,
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

    #[test]
    fn test_build_request_messages_from_config() {
        let executor = ProviderExecutor {
            provider: Arc::new(MockProvider::default_response()),
        };
        let node = ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: "m".into(),
            retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
            fallback: None,
            config: HashMap::from([
                ("messages".into(), serde_json::json!([
                    {"role": "user", "content": "hello from config"}
                ])),
                ("temperature".into(), serde_json::json!(0.7)),
                ("max_tokens".into(), serde_json::json!(512)),
            ]),
            subgraph: None,
        };
        let request = executor.build_request(&node, &NodeExecContext::default());
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].role, "user");
        assert_eq!(request.messages[0].content, "hello from config");
        assert_eq!(request.temperature, Some(0.7));
        assert_eq!(request.max_tokens, Some(512));
    }

    #[test]
    fn test_build_request_judge_gets_system_prompt_and_parent_context() {
        let executor = ProviderExecutor {
            provider: Arc::new(MockProvider::default_response()),
        };
        let parent_id = uuid::Uuid::new_v4();
        let node = ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMJudge,
            strategy: StrategyKind::Single,
            model: "judge".into(),
            retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
            fallback: None,
            config: HashMap::new(),
            subgraph: None,
        };
        let ctx = NodeExecContext {
            parent_outputs: HashMap::from([(parent_id, serde_json::json!({"answer": "42"}))]),
            graph_outputs: HashMap::new(),
        };
        let request = executor.build_request(&node, &ctx);
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
        let executor = ProviderExecutor {
            provider: Arc::new(MockProvider::default_response()),
        };
        // Config already has a system message: no injection
        let node = ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMJudge,
            strategy: StrategyKind::Single,
            model: "judge".into(),
            retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
            fallback: None,
            config: HashMap::from([
                ("messages".into(), serde_json::json!([
                    {"role": "system", "content": "custom system"}
                ])),
            ]),
            subgraph: None,
        };
        let request = executor.build_request(&node, &NodeExecContext::default());
        assert_eq!(
            request.messages.iter().filter(|m| m.role == "system").count(),
            1,
            "system prompt must be inserted exactly once"
        );
        assert_eq!(request.messages[0].content, "custom system");
    }
}