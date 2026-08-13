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
        let mut total_tokens: u64 = 0;
        let mut total_cost: f64 = 0.0;
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
                            total_tokens += usage.total_tokens as u64;
                            total_cost += usage.total_tokens as f64 * 0.000001;
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

        NodeExecutionResult {
            state: NodeState::Succeeded,
            usage: Some(Usage {
                prompt_tokens: total_tokens as u32,
                completion_tokens: 0,
                total_tokens: total_tokens as u32,
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
}

#[async_trait]
impl Executor for ProviderExecutor {
    async fn execute_node(&self, node: &ExecutionNode, ctx: &NodeExecContext) -> NodeExecutionResult {
        // Prebuilt subgraph (Phase 4.3): execute inner nodes in dependency
        // order, propagating outputs; the exit node's output is the result.
        if let Some(subgraph) = &node.subgraph {
            return self.execute_subgraph(node, subgraph, ctx).await;
        }

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

        let start = std::time::Instant::now();
        let mut last_error: Option<String> = None;

        // Primary model attempts: 1 + max_retries
        let mut attempts = 1 + node.retry_policy.max_retries;
        loop {
            if attempts == 0 {
                break;
            }
            attempts -= 1;

            let request = self.build_request(node, ctx);
            match self.provider.chat_completion(&request).await {
                Ok(response) => {
                    return NodeExecutionResult {
                        state: NodeState::Succeeded,
                        usage: response.usage,
                        latency_ms: start.elapsed().as_millis() as u64,
                        output: Some(serde_json::json!({
                            "content": response.content,
                            "node_id": node.id.to_string(),
                        })),
                    };
                }
                Err(e) => {
                    last_error = Some(e.clone());
                    if attempts > 0 && node.retry_policy.backoff_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            node.retry_policy.backoff_ms,
                        )).await;
                    }
                }
            }
        }

        // Fallback model attempts (1 try)
        if let Some(fallback) = &node.fallback {
            let mut fallback_request = self.build_request(node, ctx);
            fallback_request.model = fallback.model.clone();
            match self.provider.chat_completion(&fallback_request).await {
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
            let attempt = self.attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
            Self { fallback_attempts: std::sync::Mutex::new(Vec::new()) }
        }

        pub fn fallback_attempts(&self) -> Vec<String> {
            self.fallback_attempts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ChatProvider for FailingProvider {
        async fn chat_completion(&self, request: &ChatRequest) -> Result<ChatResponse, String> {
            self.fallback_attempts.lock().unwrap().push(request.model.clone());
            Err("always fails".into())
        }
    }

    fn make_llm_node(model: &str, retries: u32, fallback: Option<&str>) -> ExecutionNode {
        ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: model.into(),
            retry_policy: RetryPolicy { max_retries: retries, backoff_ms: 0 },
            fallback: fallback.map(|m| FallbackConfig { model: m.into(), provider: "fallback".into() }),
            config: HashMap::new(),
            subgraph: None,
        }
    }

    #[tokio::test]
    async fn test_retry_succeeds_on_second_attempt() {
        let provider = Arc::new(FlakyProvider::new(1));
        let executor = ProviderExecutor { provider: provider.clone() };
        let node = make_llm_node("primary-model", 2, None);
        let result = executor.execute_node(&node, &NodeExecContext::default()).await;
        assert!(matches!(result.state, NodeState::Succeeded), "retry should succeed");
        assert_eq!(provider.attempt_count(), 2, "2 attempts: 1 fail + 1 success");
    }

    #[tokio::test]
    async fn test_retries_exhausted_then_fallback_model_used() {
        let provider = Arc::new(FailingProvider::new());
        let executor = ProviderExecutor { provider: provider.clone() };
        let node = make_llm_node("primary-model", 1, Some("fallback-model"));
        let result = executor.execute_node(&node, &NodeExecContext::default()).await;
        // FailingProvider always fails, so node itself fails — but we must
        // verify the fallback model was attempted.
        let attempts = provider.fallback_attempts();
        assert!(attempts.contains(&"primary-model".to_string()), "primary model must be tried");
        assert!(attempts.contains(&"fallback-model".to_string()), "fallback model must be tried after retries");
        assert_eq!(attempts.len(), 3, "1 primary + 1 retry + 1 fallback");
        assert!(matches!(result.state, NodeState::Failed(_)));
    }

    #[tokio::test]
    async fn test_no_fallback_fails_after_retries() {
        let provider = Arc::new(FailingProvider::new());
        let executor = ProviderExecutor { provider: provider.clone() };
        let node = make_llm_node("primary-model", 1, None);
        let result = executor.execute_node(&node, &NodeExecContext::default()).await;
        assert!(matches!(result.state, NodeState::Failed(_)));
        assert_eq!(provider.fallback_attempts().len(), 2, "1 primary + 1 retry, no fallback");
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
                        usage: Some(Usage { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 }),
                    })
                } else {
                    Err("primary down".into())
                }
            }
        }

        let provider = Arc::new(FallbackSucceedsProvider { attempts: std::sync::Mutex::new(Vec::new()) });
        let executor = ProviderExecutor { provider: provider.clone() };
        let node = make_llm_node("primary-model", 0, Some("fallback-model"));
        let result = executor.execute_node(&node, &NodeExecContext::default()).await;
        assert!(matches!(result.state, NodeState::Succeeded), "fallback must save the node");
        let attempts = provider.attempts.lock().unwrap().clone();
        assert_eq!(attempts, vec!["primary-model".to_string(), "fallback-model".to_string()]);
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
                    retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: member_b,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "member_b".into(),
                    retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: judge,
                    kind: ExecutionNodeKind::LLMJudge,
                    strategy: StrategyKind::Single,
                    model: "judge".into(),
                    retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
            ],
            edges: vec![
                ExecutionEdge { from: member_a, to: judge, condition: None },
                ExecutionEdge { from: member_b, to: judge, condition: None },
            ],
            entry_node_id: member_a,
            exit_node_id: judge,
        };
        let node = ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Consensus,
            model: "consensus".into(),
            retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
            fallback: None,
            config: HashMap::new(),
            subgraph: Some(subgraph),
        };
        (node, spy)
    }

    #[tokio::test]
    async fn test_subgraph_executes_members_and_judge() {
        let (node, spy) = make_consensus_subgraph();
        let executor = ProviderExecutor { provider: spy.clone() };
        let result = executor.execute_node(&node, &NodeExecContext::default()).await;
        assert!(matches!(result.state, NodeState::Succeeded), "subgraph must succeed");
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
            messages.iter().any(|m| m.role == "user" && m.content.contains("Context from parent node")),
            "judge must see member outputs"
        );
        assert!(
            messages.iter().any(|m| m.role == "system" && m.content.contains("judge")),
            "judge must have system prompt"
        );
    }

    #[tokio::test]
    async fn test_subgraph_failure_propagates() {
        let spy = Arc::new(FailingProvider::new());
        let (mut node, _) = make_consensus_subgraph();
        node.subgraph.as_mut().unwrap().nodes[0].retry_policy = RetryPolicy { max_retries: 0, backoff_ms: 0 };
        let executor = ProviderExecutor { provider: spy };
        let result = executor.execute_node(&node, &NodeExecContext::default()).await;
        assert!(matches!(result.state, NodeState::Failed(_)), "subgraph member failure must propagate");
    }

    // -----------------------------------------------------------------------
    // Phase 4.4: control-flow kinds — Gate must not call provider
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_gate_node_does_not_call_provider() {
        let spy = Arc::new(SpyProvider::new());
        let executor = ProviderExecutor { provider: spy.clone() };
        let node = ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: ExecutionNodeKind::Gate,
            strategy: StrategyKind::Single,
            model: "policy.approval_gate".into(),
            retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
            fallback: None,
            config: HashMap::new(),
            subgraph: None,
        };
        let result = executor.execute_node(&node, &NodeExecContext::default()).await;
        assert!(matches!(result.state, NodeState::Succeeded), "gate must succeed");
        assert!(spy.last_request().is_none(), "gate must NOT call the LLM provider");
    }
}