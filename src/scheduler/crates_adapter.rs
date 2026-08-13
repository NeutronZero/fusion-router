//! Crates-facing executor adapter for the src scheduler flip (Phase 6.3b).
//!
//! `CratesExecutorAdapter` exposes a src `Executor` to `fusion_scheduler` as
//! a `fusion_scheduler::Executor`. It re-homes the two behaviors the monolith
//! scheduler used to own in `run_inner`:
//!
//! - **Retry / fallback**: nodes are retried up to `retry_policy.max_retries`
//!   with exponential backoff, then a `node.fallback` model is attempted — a
//!   port of `src/scheduler/default.rs::run_inner` (Failed branch). The crates
//!   scheduler stays pure; `fusion_runtime::ProviderExecutor` already retries
//!   on its own path, so nothing double-retries.
//! - **Terminal tracking**: the src `ExecutionInstance` exposes
//!   `terminal_node_id` / `final_output` (last succeeded node, last non-null
//!   output); the crates `ExecutionOutcome` does not, so the adapter records
//!   them on each success.
//!
//! Divergence (strictly better): a failure with reason `"Cancelled by client"`
//! is never retried, since the crates loop already races the cancellation
//! token per node.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tracing::info;
use uuid::Uuid;

use crate::transport::backoff::Backoff;
use crate::types::{ExecutionNode, NodeExecutionResult, NodeState};

const CANCELLED_MARKER: &str = "Cancelled by client";

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
        (Self { inner, tracker: tracker.clone() }, tracker)
    }
}

#[async_trait]
impl fusion_scheduler::Executor for CratesExecutorAdapter<'_> {
    async fn execute_node(
        &self,
        node: &ExecutionNode,
        _ctx: &fusion_types::NodeExecContext,
    ) -> NodeExecutionResult {
        let max_retries = node.retry_policy.max_retries;
        let mut attempts: u32 = 0;
        let mut backoff: Option<Backoff> = None;

        loop {
            let result = self.inner.execute_node(node).await;
            match result.state {
                NodeState::Succeeded => {
                    self.tracker
                        .lock()
                        .unwrap()
                        .record_success(node.id, result.output.as_ref());
                    return result;
                }
                NodeState::Failed(ref reason) if reason == CANCELLED_MARKER => return result,
                NodeState::Failed(reason) => {
                    if attempts < max_retries {
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
                    if let Some(ref fallback) = node.fallback {
                        info!(
                            node_id = ?node.id,
                            fallback_model = %fallback.model,
                            "Attempting fallback execution"
                        );
                        let mut fallback_node = node.clone();
                        fallback_node.model = fallback.model.clone();
                        let fb_result = self.inner.execute_node(&fallback_node).await;
                        return match fb_result.state {
                            NodeState::Succeeded => {
                                self.tracker.lock().unwrap().record_success(
                                    node.id,
                                    fb_result.output.as_ref(),
                                );
                                NodeExecutionResult {
                                    state: NodeState::Succeeded,
                                    usage: fb_result.usage,
                                    latency_ms: fb_result.latency_ms,
                                    output: fb_result.output,
                                }
                            }
                            NodeState::Failed(fb_reason) => NodeExecutionResult {
                                state: NodeState::Failed(format!("Fallback failed: {}", fb_reason)),
                                usage: fb_result.usage,
                                latency_ms: fb_result.latency_ms,
                                output: None,
                            },
                            other => NodeExecutionResult {
                                state: other,
                                usage: fb_result.usage,
                                latency_ms: fb_result.latency_ms,
                                output: fb_result.output,
                            },
                        };
                    }
                    return NodeExecutionResult {
                        state: NodeState::Failed(reason),
                        usage: result.usage,
                        latency_ms: result.latency_ms,
                        output: result.output,
                    };
                }
                other => {
                    return NodeExecutionResult {
                        state: other,
                        usage: result.usage,
                        latency_ms: result.latency_ms,
                        output: result.output,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use fusion_scheduler::Executor as _;

    use crate::types::{
        ExecutionNodeKind, RetryPolicy, StrategyKind, Usage,
    };

    fn make_node(retries: u32, fallback: Option<String>) -> ExecutionNode {
        ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: "primary-model".into(),
            retry_policy: RetryPolicy {
                max_retries: retries,
                backoff_ms: 0,
            },
            fallback: fallback.map(|model| crate::types::FallbackConfig {
                model,
                provider: "fallback-provider".into(),
            }),
            config: HashMap::new(),
            subgraph: None,
        }
    }

    struct ScriptedExecutor {
        calls: AtomicUsize,
        fail_first: usize,
        fail_models: Vec<String>,
        recorded_models: Mutex<Vec<String>>,
    }

    impl ScriptedExecutor {
        fn fail_then_succeed(fail_first: usize) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                fail_first,
                fail_models: Vec::new(),
                recorded_models: Mutex::new(Vec::new()),
            }
        }

        fn fail_models(fail_models: Vec<&str>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                fail_first: 0,
                fail_models: fail_models.into_iter().map(String::from).collect(),
                recorded_models: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl crate::executor::Executor for ScriptedExecutor {
        async fn execute_node(&self, node: &ExecutionNode) -> NodeExecutionResult {
            self.recorded_models.lock().unwrap().push(node.model.clone());
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            let model_fails = self.fail_models.iter().any(|m| m == &node.model);
            if model_fails || n < self.fail_first {
                return NodeExecutionResult {
                    state: NodeState::Failed("provider error".into()),
                    usage: None,
                    latency_ms: 1,
                    output: None,
                };
            }
            NodeExecutionResult {
                state: NodeState::Succeeded,
                usage: Some(Usage {
                    prompt_tokens: 5,
                    completion_tokens: 5,
                    total_tokens: 10,
                }),
                latency_ms: 2,
                output: Some(serde_json::json!({"answer": "ok"})),
            }
        }

        async fn resolve_strategy(&self, _node: &ExecutionNode) -> crate::types::ExecutionSubgraph {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn retries_then_succeeds() {
        let inner = ScriptedExecutor::fail_then_succeed(2);
        let (adapter, _tracker) = CratesExecutorAdapter::new(&inner);
        let node = make_node(2, None);

        let result = adapter
            .execute_node(&node, &fusion_types::NodeExecContext::default())
            .await;
        assert_eq!(result.state, NodeState::Succeeded);
        assert_eq!(inner.calls.load(Ordering::SeqCst), 3, "2 failures + 1 success");
    }

    #[tokio::test]
    async fn retries_exhausted_then_fallback_succeeds() {
        let inner = ScriptedExecutor::fail_models(vec!["primary-model"]);
        let (adapter, _tracker) = CratesExecutorAdapter::new(&inner);
        // max_retries=1 -> 2 primary attempts, then the fallback replaces the model.
        let node = make_node(1, Some("fallback-model".into()));

        let result = adapter
            .execute_node(&node, &fusion_types::NodeExecContext::default())
            .await;
        assert_eq!(result.state, NodeState::Succeeded, "fallback must satisfy the node");
        let models = inner.recorded_models.lock().unwrap().clone();
        assert_eq!(models.len(), 3);
        assert_eq!(&models[0], "primary-model");
        assert_eq!(&models[2], "fallback-model");
    }

    #[tokio::test]
    async fn fallback_failure_propagates_client_message() {
        let inner = ScriptedExecutor::fail_models(vec!["primary-model", "fallback-model"]);
        let (adapter, _tracker) = CratesExecutorAdapter::new(&inner);
        let node = make_node(0, Some("fallback-model".into()));

        let result = adapter
            .execute_node(&node, &fusion_types::NodeExecContext::default())
            .await;
        match result.state {
            NodeState::Failed(reason) => {
                assert!(reason.starts_with("Fallback failed:"), "got: {reason}");
            }
            other => panic!("expected failed state, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn exhausted_retries_without_fallback_return_original_failure() {
        let inner = ScriptedExecutor::fail_models(vec!["primary-model"]);
        let (adapter, _tracker) = CratesExecutorAdapter::new(&inner);
        let node = make_node(3, None);

        let result = adapter
            .execute_node(&node, &fusion_types::NodeExecContext::default())
            .await;
        assert!(matches!(result.state, NodeState::Failed(reason) if reason == "provider error"));
        assert_eq!(inner.calls.load(Ordering::SeqCst), 4, "initial + 3 retries");
    }

    #[tokio::test]
    async fn cancellation_failure_is_never_retried() {
        struct CancelledExecutor(Mutex<AtomicUsize>);
        #[async_trait]
        impl crate::executor::Executor for CancelledExecutor {
            async fn execute_node(&self, _node: &ExecutionNode) -> NodeExecutionResult {
                self.0.lock().unwrap().fetch_add(1, Ordering::SeqCst);
                NodeExecutionResult {
                    state: NodeState::Failed("Cancelled by client".into()),
                    usage: None,
                    latency_ms: 0,
                    output: None,
                }
            }

            async fn resolve_strategy(&self, _node: &ExecutionNode) -> crate::types::ExecutionSubgraph {
                unimplemented!()
            }
        }

        let inner = CancelledExecutor(Mutex::new(AtomicUsize::new(0)));
        let (adapter, _tracker) = CratesExecutorAdapter::new(&inner);
        let node = make_node(5, Some("fallback-model".into()));

        let result = adapter
            .execute_node(&node, &fusion_types::NodeExecContext::default())
            .await;
        assert_eq!(inner.0.lock().unwrap().load(Ordering::SeqCst), 1, "cancellation must pass through untouched");
        assert!(matches!(result.state, NodeState::Failed(reason) if reason == "Cancelled by client"));
    }

    #[tokio::test]
    async fn tracks_terminal_and_final_output() {
        struct SequentialExecutor;
        #[async_trait]
        impl crate::executor::Executor for SequentialExecutor {
            async fn execute_node(&self, node: &ExecutionNode) -> NodeExecutionResult {
                NodeExecutionResult {
                    state: NodeState::Succeeded,
                    usage: None,
                    latency_ms: 1,
                    output: Some(serde_json::json!(format!("out-{}", node.id))),
                }
            }

            async fn resolve_strategy(&self, _node: &ExecutionNode) -> crate::types::ExecutionSubgraph {
                unimplemented!()
            }
        }

        let (adapter, tracker) = CratesExecutorAdapter::new(&SequentialExecutor);
        let n1 = make_node(0, None);
        let n2 = make_node(0, None);
        adapter
            .execute_node(&n1, &fusion_types::NodeExecContext::default())
            .await;
        adapter
            .execute_node(&n2, &fusion_types::NodeExecContext::default())
            .await;

        let t = tracker.lock().unwrap();
        assert_eq!(t.terminal_node_id, Some(n2.id), "last success must be terminal");
        assert_eq!(t.final_output, Some(serde_json::json!(format!("out-{}", n2.id))));
    }
}