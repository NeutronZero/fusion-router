//! `LocalEri` — a local `ExecutionRuntimeInterface` implementation.
//!
//! Executes `ExecutionAbi` documents against the live v0.12 engine
//! (`DefaultScheduler` + `DefaultExecutor`), which is the only production
//! execution path. The ABI is provider-free by contract; this runtime binds
//! every node to the `model` supplied at construction. Execution is
//! synchronous and in-process; `state()` reports recorded outcomes for
//! completed executions and `cancel()` fails closed (nothing is ever
//! in flight in this implementation).

use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::abi::to_graph::graph_from_abi;
use crate::abi::{ExecutionAbi, EXECUTION_ABI_VERSION};
use crate::eri::{EriError, ExecutionAbiResult, ExecutionRuntimeInterface, ExecutionState};
use crate::executor::DefaultExecutor;
use crate::providers::ChatProvider;
use crate::scheduler::default::DefaultScheduler;
use crate::scheduler::Scheduler;
use crate::target::ExecutionTarget;
use crate::types::{ReservationId, StrategyKind};

/// Local, in-process ERI backed by the production scheduler and executor.
pub struct LocalEri {
    scheduler: DefaultScheduler,
    executor: DefaultExecutor,
    model: String,
    states: RwLock<HashMap<Uuid, ExecutionState>>,
}

impl LocalEri {
    pub fn new(
        provider: Arc<dyn ChatProvider + Send + Sync>,
        model: impl Into<String>,
        strategies: HashMap<StrategyKind, Box<dyn crate::strategies::Strategy + Send + Sync>>,
    ) -> Self {
        Self {
            scheduler: DefaultScheduler::default(),
            executor: DefaultExecutor::new(provider, strategies),
            model: model.into(),
            states: RwLock::new(HashMap::new()),
        }
    }

    fn record(&self, execution_id: Uuid, state: ExecutionState) {
        self.states.write().insert(execution_id, state);
    }
}

#[async_trait]
impl ExecutionRuntimeInterface for LocalEri {
    fn name(&self) -> &'static str {
        "fusion-local"
    }

    async fn execute(
        &self,
        abi: &ExecutionAbi,
        target: &ExecutionTarget,
    ) -> Result<ExecutionAbiResult, EriError> {
        if abi.version != EXECUTION_ABI_VERSION {
            return Err(EriError::UnsupportedAbiVersion(abi.version));
        }
        if target.environment != crate::target::ExecutionEnvironment::Local {
            return Err(EriError::ExecutionFailed(format!(
                "LocalEri only supports ExecutionEnvironment::Local, got {:?}",
                target.environment
            )));
        }

        let graph = graph_from_abi(abi, &self.model)
            .map_err(EriError::ExecutionFailed)?;

        let mut instance = self.scheduler.schedule(graph, ReservationId(Uuid::new_v4()));
        let execution_id = instance.instance_id;
        let result = self
            .scheduler
            .run(&mut instance, &self.executor)
            .await
            .map_err(|err| EriError::ExecutionFailed(err.to_string()))?;

        let state = if result.success {
            ExecutionState::Succeeded
        } else {
            ExecutionState::Failed
        };
        self.record(execution_id, state);

        let outputs = instance
            .outputs
            .into_iter()
            .map(|(id, value)| (id.to_string(), value))
            .collect();

        Ok(ExecutionAbiResult {
            execution_id,
            state,
            outputs,
            metrics: HashMap::from([
                ("total_latency_ms".to_string(), result.total_latency_ms as f64),
                ("total_cost_usd".to_string(), result.total_cost.to_usd_f64()),
                ("total_tokens".to_string(), result.total_tokens as f64),
            ]),
        })
    }

    async fn cancel(&self, execution_id: &Uuid) -> Result<(), EriError> {
        let mut states = self.states.write();
        match states.get(execution_id) {
            Some(ExecutionState::Succeeded | ExecutionState::Failed) => {
                Err(EriError::ExecutionFailed(
                    "execution already completed; nothing to cancel".into(),
                ))
            }
            Some(_) => {
                states.insert(*execution_id, ExecutionState::Cancelled);
                Ok(())
            }
            None => Err(EriError::NotFound(*execution_id)),
        }
    }

    async fn state(&self, execution_id: &Uuid) -> Result<ExecutionState, EriError> {
        self.states
            .read()
            .get(execution_id)
            .copied()
            .ok_or(EriError::NotFound(*execution_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::from_graph::abi_from_graph;
    use crate::strategies::single::SingleStrategy;
    use crate::types::{
        ChatCompletionRequest, ChatCompletionResponse, Choice, ChatMessage, ExecutionGraph,
        ExecutionNode, ExecutionNodeKind, GraphMetadata, RetryPolicy, Usage,
    };
    use crate::providers::ChatProvider;

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
                native_tool_calls: None,
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

    fn single_node_graph() -> ExecutionGraph {
        let node = ExecutionNode {
            id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440011").unwrap(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: "mock-model".into(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config: HashMap::new(),
            subgraph: None,
        };
        ExecutionGraph {
            graph_id: Uuid::new_v4(),
            nodes: vec![node],
            edges: vec![],
            metadata: GraphMetadata {
                estimated_cost: crate::types::NanoUSD::ZERO,
                estimated_tokens: 0,
                max_depth: 1,
                node_count: 1,
            },
            total_tokens: 0,
            total_cost: crate::types::NanoUSD::ZERO,
            primitive_graph_hash: 0,
        }
    }

    fn local_eri() -> LocalEri {
        LocalEri::new(
            Arc::new(MockChatProvider),
            "mock-model",
            HashMap::from([(StrategyKind::Single, Box::new(SingleStrategy) as Box<dyn crate::strategies::Strategy + Send + Sync>)]),
        )
    }

    #[tokio::test]
    async fn executes_abi_to_success() {
        let abi = abi_from_graph(&single_node_graph());
        let eri = local_eri();
        let result = eri.execute(&abi, &ExecutionTarget::default()).await.unwrap();
        assert_eq!(result.state, ExecutionState::Succeeded);
        assert!(!result.outputs.is_empty());
        assert_eq!(
            eri.state(&result.execution_id).await.unwrap(),
            ExecutionState::Succeeded
        );
        assert!(result.metrics["total_tokens"] > 0.0);
    }

    #[tokio::test]
    async fn rejects_unsupported_abi_version() {
        let mut abi = abi_from_graph(&single_node_graph());
        abi.version = 99;
        let err = local_eri().execute(&abi, &ExecutionTarget::default()).await.unwrap_err();
        assert!(matches!(err, EriError::UnsupportedAbiVersion(99)));
    }

    #[tokio::test]
    async fn rejects_non_local_target() {
        let abi = abi_from_graph(&single_node_graph());
        let mut target = ExecutionTarget::default();
        target.environment = crate::target::ExecutionEnvironment::Cloud;
        let err = local_eri().execute(&abi, &target).await.unwrap_err();
        assert!(matches!(err, EriError::ExecutionFailed(_)));
    }

    #[tokio::test]
    async fn state_and_cancel_for_unknown_ids() {
        let eri = local_eri();
        let unknown = Uuid::new_v4();
        assert!(matches!(eri.state(&unknown).await, Err(EriError::NotFound(_))));
        assert!(matches!(eri.cancel(&unknown).await, Err(EriError::NotFound(_))));
    }

    #[tokio::test]
    async fn cancel_completed_execution_fails_closed() {
        let abi = abi_from_graph(&single_node_graph());
        let eri = local_eri();
        let result = eri.execute(&abi, &ExecutionTarget::default()).await.unwrap();
        let err = eri.cancel(&result.execution_id).await.unwrap_err();
        assert!(matches!(err, EriError::ExecutionFailed(_)));
    }
}