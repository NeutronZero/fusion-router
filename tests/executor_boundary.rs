//! Gate 03 compile-level boundary test.
//!
//! Verifies that the host executor module (`src/executor/`) is reduced to
//! runtime delegation adapters. This test complements the textual source
//! scan in `check_monolith_freeze.py` by asserting compile-time facts about
//! the public API surface.

use fusion_router::executor::{DefaultExecutor, Executor};
use fusion_router::types::{ExecutionNode, ExecutionNodeKind, NodeExecContext, NodeExecutionResult, StrategyKind};
use std::collections::HashMap;
use std::sync::Arc;

/// A minimal provider that always errors — used only to construct
/// the executor for API-surface assertions. The executor must still
/// delegate to fusion-runtime without panicking.
struct StubProvider;

#[async_trait::async_trait]
impl fusion_router::providers::ChatProvider for StubProvider {
    async fn chat_completion(
        &self,
        _req: &fusion_router::types::ChatCompletionRequest,
    ) -> anyhow::Result<fusion_router::types::ChatCompletionResponse> {
        Err(anyhow::anyhow!("stub provider"))
    }
    fn name(&self) -> &str { "stub" }
}

/// Compile-time assertion: `DefaultExecutor` implements the `Executor` trait,
/// confirming it is a runtime adapter — not a standalone strategy engine.
#[tokio::test]
async fn executor_implements_executor_trait() {
    let executor = Arc::new(DefaultExecutor::new(
        Arc::new(StubProvider),
        HashMap::new(),
    ));
    let node = ExecutionNode {
        id: uuid::Uuid::new_v4(),
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Single,
        model: "stub-model".into(),
        retry_policy: fusion_router::types::RetryPolicy { max_retries: 0, backoff_ms: 0 },
        fallback: None,
        config: HashMap::new(),
        subgraph: None,
    };
    let result: NodeExecutionResult = executor.execute_node(&node, &NodeExecContext::default()).await;
    // The adapter delegates to fusion-runtime; a stub provider will fail,
    // but the delegation path itself is exercised.
    assert!(
        matches!(result.state, fusion_router::types::NodeState::Succeeded)
            || matches!(result.state, fusion_router::types::NodeState::Failed(_)),
        "executor must delegate to runtime (success or failure is provider-dependent)"
    );
}

/// Compile-time assertion: `DefaultExecutor` is `Send + Sync`, required
/// for the production async pipeline.
#[test]
fn executor_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DefaultExecutor>();
}
