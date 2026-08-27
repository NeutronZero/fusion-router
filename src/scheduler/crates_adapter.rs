//! Crates-facing executor adapter for the src scheduler flip (Phase 6.3b,
//! slimmed in 6.4).
//!
//! `CratesExecutorAdapter` exposes a src `Executor` to `fusion_scheduler` as
//! a `fusion_scheduler::Executor`.
//!
//! Since 6.4 it is a pure forwarder:
//!
//! - **Retry / fallback** moved into the src executor boundary
//!   (`DefaultExecutor`): plain Single leaves run on
//!   `fusion_runtime::ProviderExecutor` (which owns retry/fallback), and the
//!   legacy strategy/tool path owns its own retry loop. Nothing double-retries.
//! - **Terminal tracking** stays here: the src `ExecutionInstance` exposes
//!   `terminal_node_id` / `final_output` (last succeeded node, last non-null
//!   output); the crates `ExecutionOutcome` does not, so the adapter records
//!   them on each success.
//!
//! Cancellation is handled entirely by the crates loop (it races the token
//! per node and sets `Failed("Cancelled by client")` itself), so the adapter
//! needs no special casing.

use std::sync::Arc;
use parking_lot::Mutex;

use async_trait::async_trait;
use uuid::Uuid;

use crate::types::{ExecutionNode, NodeExecContext, NodeExecutionResult, NodeState};

/// Last-success terminal output bookkeeping for the src `ExecutionInstance`.
#[derive(Debug, Default)]
pub(crate) struct OutputTracker {
    pub terminal_node_id: Option<Uuid>,
    pub final_output: Option<serde_json::Value>,
}

impl OutputTracker {
    fn record_success(&mut self, node_id: Uuid, output: Option<&serde_json::Value>) {
        self.terminal_node_id = Some(node_id);
        if let Some(out) = output {
            if *out != serde_json::Value::Null {
                self.final_output = Some(out.clone());
            }
        }
    }
}

pub(crate) struct CratesExecutorAdapter<'a> {
    inner: &'a dyn crate::executor::Executor,
    tracker: Arc<Mutex<OutputTracker>>,
}

impl<'a> CratesExecutorAdapter<'a> {
    pub fn new(inner: &'a dyn crate::executor::Executor) -> (Self, Arc<Mutex<OutputTracker>>) {
        let tracker = Arc::new(Mutex::new(OutputTracker::default()));
        (
            Self {
                inner,
                tracker: tracker.clone(),
            },
            tracker,
        )
    }
}

#[async_trait]
impl fusion_scheduler::Executor for CratesExecutorAdapter<'_> {
    async fn execute_node(
        &self,
        node: &ExecutionNode,
        ctx: &NodeExecContext,
    ) -> NodeExecutionResult {
        let result = self.inner.execute_node(node, ctx).await;
        if result.state == NodeState::Succeeded {
            self.tracker
                .lock()
                .record_success(node.id, result.output.as_ref());
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use fusion_scheduler::Executor as _;

    use crate::types::{ExecutionNodeKind, RetryPolicy, StrategyKind};

    fn make_node() -> ExecutionNode {
        ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: "primary-model".into(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config: HashMap::new(),
            subgraph: None,
        }
    }

    struct SequentialExecutor;

    #[async_trait]
    impl crate::executor::Executor for SequentialExecutor {
        async fn execute_node(
            &self,
            node: &ExecutionNode,
            ctx: &NodeExecContext,
        ) -> NodeExecutionResult {
            let parent_count = ctx.parent_outputs.len();
            NodeExecutionResult {
                state: NodeState::Succeeded,
                usage: None,
                latency_ms: 1,
                output: Some(serde_json::json!(format!(
                    "out-{} (parents: {})",
                    node.id, parent_count
                ))),
            }
        }
    }

    #[tokio::test]
    async fn forwards_ctx_and_result_unchanged() {
        let (adapter, _tracker) = CratesExecutorAdapter::new(&SequentialExecutor);
        let node = make_node();
        let ctx = NodeExecContext {
            parent_outputs: HashMap::from([(Uuid::new_v4(), serde_json::json!({"answer": 42}))]),
            graph_outputs: HashMap::new(),
        };

        let result = adapter.execute_node(&node, &ctx).await;

        assert_eq!(result.state, NodeState::Succeeded);
        let output = result.output.expect("output");
        assert_eq!(
            output,
            serde_json::json!(format!("out-{} (parents: 1)", node.id)),
            "ctx must reach the inner executor untouched"
        );
    }

    #[tokio::test]
    async fn tracks_terminal_and_final_output() {
        let (adapter, tracker) = CratesExecutorAdapter::new(&SequentialExecutor);
        let n1 = make_node();
        let n2 = make_node();
        adapter.execute_node(&n1, &NodeExecContext::default()).await;
        adapter.execute_node(&n2, &NodeExecContext::default()).await;

        let t = tracker.lock();
        assert_eq!(
            t.terminal_node_id,
            Some(n2.id),
            "last success must be terminal"
        );
        assert_eq!(
            t.final_output,
            Some(serde_json::json!(format!("out-{} (parents: 0)", n2.id)))
        );
    }

    #[tokio::test]
    async fn failure_does_not_update_terminal() {
        struct FailingExecutor;
        #[async_trait]
        impl crate::executor::Executor for FailingExecutor {
            async fn execute_node(
                &self,
                _node: &ExecutionNode,
                _ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
                NodeExecutionResult {
                    state: NodeState::Failed("boom".into()),
                    usage: None,
                    latency_ms: 1,
                    output: None,
                }
            }
        }

        let (adapter, tracker) = CratesExecutorAdapter::new(&FailingExecutor);
        adapter
            .execute_node(&make_node(), &NodeExecContext::default())
            .await;

        let t = tracker.lock();
        assert_eq!(t.terminal_node_id, None);
        assert_eq!(t.final_output, None);
    }
}
