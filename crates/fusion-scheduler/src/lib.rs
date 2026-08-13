//! DAG scheduler with WorkQueue for topological execution.
//!
//! The `WorkQueue` maintains execution state and provides ready-node selection.
//! The `DefaultScheduler` runs a DAG to completion using an `Executor` trait.

use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use fusion_types::*;
use fusion_core::PlatformError;

pub mod work_queue;
pub use work_queue::WorkQueue;

/// Trait for executing a single node. Implementors provide the actual
/// LLM/provider dispatch. The scheduler calls this for each ready node,
/// passing a context with parent outputs.
#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute_node(&self, node: &ExecutionNode, ctx: &NodeExecContext) -> NodeExecutionResult;
}

/// Result of a full scheduler run.
#[derive(Debug)]
pub struct ExecutionOutcome {
    pub success: bool,
    pub outputs: HashMap<uuid::Uuid, serde_json::Value>,
    pub total_latency_ms: u64,
    pub total_cost: f64,
    pub total_tokens: u64,
}

/// DAG scheduler that executes nodes respecting dependency order.
pub struct DefaultScheduler {
    max_concurrent: usize,
}

impl DefaultScheduler {
    pub fn new() -> Self {
        Self { max_concurrent: 16 }
    }

    pub fn with_max_concurrent(max_concurrent: usize) -> Self {
        Self { max_concurrent }
    }

    /// Execute a graph to completion using the provided executor.
    pub async fn run(
        &self,
        graph: Arc<ExecutionGraph>,
        executor: &dyn Executor,
    ) -> Result<ExecutionOutcome, PlatformError> {
        let mut queue = WorkQueue::new(graph.clone());
        let mut node_states: HashMap<uuid::Uuid, NodeState> = HashMap::new();
        let mut outputs: HashMap<uuid::Uuid, serde_json::Value> = HashMap::new();
        let start = std::time::Instant::now();
        let mut total_cost: f64 = 0.0;
        let mut total_tokens: u64 = 0;

        loop {
            if queue.is_done(&node_states) {
                break;
            }

            let ready = queue.get_ready(&node_states);
            if ready.is_empty() {
                if queue.any_in_progress() {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    continue;
                }
                break;
            }

            // Collect node IDs first to release immutable borrow on queue
            let batch_ids: Vec<uuid::Uuid> = ready.into_iter()
                .take(self.max_concurrent)
                .map(|n| n.id)
                .collect();

            // Now we can mutate queue
            let mut handles = Vec::new();
            for node_id in batch_ids {
                queue.mark_in_progress(node_id);
                node_states.insert(node_id, NodeState::Running);

                // Find the node to clone for the executor
                let node = graph.nodes.iter().find(|n| n.id == node_id).unwrap().clone();

                // Build parent context: outputs of immediate predecessors
                let incoming: Vec<uuid::Uuid> = graph.edges.iter()
                    .filter(|e| e.to == node_id)
                    .map(|e| e.from)
                    .collect();
                let mut parent_outputs = HashMap::new();
                for parent_id in incoming {
                    if let Some(out) = outputs.get(&parent_id) {
                        parent_outputs.insert(parent_id, out.clone());
                    }
                }
                let ctx = NodeExecContext {
                    parent_outputs,
                    graph_outputs: outputs.clone(),
                };

                let executor_ref = executor;
                handles.push(async move {
                    let result = executor_ref.execute_node(&node, &ctx).await;
                    (node.id, result)
                });
            }

            // Wait for all nodes in this batch
            for handle in handles {
                let (node_id, result) = handle.await;
                match result.state {
                    NodeState::Succeeded => {
                        queue.mark_completed(node_id);
                        node_states.insert(node_id, NodeState::Succeeded);
                        if let Some(output) = result.output {
                            outputs.insert(node_id, output);
                        }
                        if let Some(ref usage) = result.usage {
                            total_tokens += usage.total_tokens as u64;
                            total_cost += usage.total_tokens as f64 * 0.000001;
                        }
                    }
                    NodeState::Failed(msg) => {
                        queue.mark_failed(node_id);
                        node_states.insert(node_id, NodeState::Failed(msg));
                    }
                    _ => {
                        queue.mark_completed(node_id);
                        node_states.insert(node_id, result.state);
                    }
                }
            }
        }

        let total_latency_ms = start.elapsed().as_millis() as u64;
        let success = graph.nodes.iter().all(|n| {
            matches!(
                node_states.get(&n.id),
                Some(NodeState::Succeeded) | Some(NodeState::Skipped)
            )
        });

        Ok(ExecutionOutcome {
            success,
            outputs,
            total_latency_ms,
            total_cost,
            total_tokens,
        })
    }
}

impl Default for DefaultScheduler {
    fn default() -> Self {
        Self::new()
    }
}

// Keep the placement-based scheduler for backward compatibility
pub use fusion_placement::{ExecutionPlan, ExecutionPlanId, PlacementGraph};

pub struct DistributedScheduler;

impl DistributedScheduler {
    pub fn new() -> Self { Self }

    pub fn create_plan(&self, placement_graph: &PlacementGraph) -> ExecutionPlan {
        let mut execution_order = Vec::new();
        let mut worker_assignments = HashMap::new();

        for node in &placement_graph.nodes {
            execution_order.push(node.id.clone());
            worker_assignments.insert(node.id.clone(), node.worker_id.clone());
        }

        ExecutionPlan {
            plan_id: ExecutionPlanId::new(),
            placement_id: placement_graph.placement_id.clone(),
            execution_id: placement_graph.execution_id.clone(),
            execution_order,
            worker_assignments,
            max_parallelism: 8,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

impl Default for DistributedScheduler {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_placement::PlacementNode;

    #[test]
    fn test_distributed_scheduler_creates_execution_plan() {
        let placement_graph = PlacementGraph {
            placement_id: fusion_placement::PlacementId::new(),
            execution_id: "exec_500".into(),
            nodes: vec![
                PlacementNode { id: "n1".into(), worker_id: "w1".into(), config: HashMap::new() },
                PlacementNode { id: "n2".into(), worker_id: "w2".into(), config: HashMap::new() },
            ],
            placement_policy: "locality-v1".into(),
        };

        let scheduler = DistributedScheduler::new();
        let plan = scheduler.create_plan(&placement_graph);
        assert_eq!(plan.execution_id, "exec_500");
        assert_eq!(plan.execution_order.len(), 2);
    }

    struct MockExecutor;

    #[async_trait]
    impl Executor for MockExecutor {
        async fn execute_node(&self, _node: &ExecutionNode, _ctx: &NodeExecContext) -> NodeExecutionResult {
            NodeExecutionResult {
                state: NodeState::Succeeded,
                usage: Some(Usage {
                    prompt_tokens: 100,
                    completion_tokens: 50,
                    total_tokens: 150,
                }),
                latency_ms: 10,
                output: Some(serde_json::json!({"result": "ok"})),
            }
        }
    }

    #[tokio::test]
    async fn test_scheduler_runs_single_node_graph() {
        let graph = Arc::new(ExecutionGraph {
            graph_id: uuid::Uuid::new_v4(),
            nodes: vec![ExecutionNode {
                id: uuid::Uuid::new_v4(),
                kind: ExecutionNodeKind::LLMGenerate,
                strategy: StrategyKind::Single,
                model: "test-model".into(),
                retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                fallback: None,
                config: HashMap::new(),
                subgraph: None,
            }],
            edges: vec![],
            metadata: GraphMetadata {
                estimated_cost: 0.01,
                estimated_tokens: 150,
                max_depth: 1,
                node_count: 1,
            },
            total_tokens: 150,
            total_cost: 10,
            primitive_graph_hash: 0,
        });

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler.run(graph, &MockExecutor).await.unwrap();
        assert!(outcome.success);
        assert_eq!(outcome.outputs.len(), 1);
    }

    #[tokio::test]
    async fn test_scheduler_runs_sequential_graph() {
        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();
        let graph = Arc::new(ExecutionGraph {
            graph_id: uuid::Uuid::new_v4(),
            nodes: vec![
                ExecutionNode {
                    id: n1,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "m1".into(),
                    retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: n2,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "m2".into(),
                    retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
            ],
            edges: vec![ExecutionEdge {
                from: n1,
                to: n2,
                condition: None,
            }],
            metadata: GraphMetadata {
                estimated_cost: 0.02,
                estimated_tokens: 300,
                max_depth: 2,
                node_count: 2,
            },
            total_tokens: 300,
            total_cost: 20,
            primitive_graph_hash: 0,
        });

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler.run(graph, &MockExecutor).await.unwrap();
        assert!(outcome.success);
        assert_eq!(outcome.outputs.len(), 2);
    }

    #[tokio::test]
    async fn test_scheduler_handles_failure() {
        struct FailingExecutor;

        #[async_trait]
        impl Executor for FailingExecutor {
            async fn execute_node(&self, node: &ExecutionNode, _ctx: &NodeExecContext) -> NodeExecutionResult {
                if node.model == "fail" {
                    NodeExecutionResult {
                        state: NodeState::Failed("provider error".into()),
                        usage: None,
                        latency_ms: 5,
                        output: None,
                    }
                } else {
                    NodeExecutionResult {
                        state: NodeState::Succeeded,
                        usage: None,
                        latency_ms: 10,
                        output: None,
                    }
                }
            }
        }

        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();
        let graph = Arc::new(ExecutionGraph {
            graph_id: uuid::Uuid::new_v4(),
            nodes: vec![
                ExecutionNode {
                    id: n1,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "fail".into(),
                    retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: n2,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "ok".into(),
                    retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
            ],
            edges: vec![ExecutionEdge {
                from: n1,
                to: n2,
                condition: None,
            }],
            metadata: GraphMetadata {
                estimated_cost: 0.02,
                estimated_tokens: 300,
                max_depth: 2,
                node_count: 2,
            },
            total_tokens: 300,
            total_cost: 20,
            primitive_graph_hash: 0,
        });

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler.run(graph, &FailingExecutor).await.unwrap();
        assert!(!outcome.success);
    }

    #[tokio::test]
    async fn test_scheduler_passes_parent_context() {
        use std::sync::Mutex;
        static CAPTURED: Mutex<Option<(uuid::Uuid, serde_json::Value)>> = Mutex::new(None);

        struct ContextCapturingExecutor;
        #[async_trait]
        impl Executor for ContextCapturingExecutor {
            async fn execute_node(&self, node: &ExecutionNode, ctx: &NodeExecContext) -> NodeExecutionResult {
                if node.model == "child" {
                    let parent = ctx.parent_outputs.iter().next().map(|(k, v)| (*k, v.clone()));
                    *CAPTURED.lock().unwrap() = parent;
                }
                NodeExecutionResult {
                    state: NodeState::Succeeded,
                    usage: None,
                    latency_ms: 0,
                    output: Some(serde_json::json!({"child": "done"})),
                }
            }
        }

        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();
        let graph = Arc::new(ExecutionGraph {
            graph_id: uuid::Uuid::new_v4(),
            nodes: vec![
                ExecutionNode {
                    id: n1,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "parent".into(),
                    retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: n2,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "child".into(),
                    retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
            ],
            edges: vec![ExecutionEdge { from: n1, to: n2, condition: None }],
            metadata: GraphMetadata {
                estimated_cost: 0.0,
                estimated_tokens: 0,
                max_depth: 2,
                node_count: 2,
            },
            total_tokens: 0,
            total_cost: 0,
            primitive_graph_hash: 0,
        });

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler.run(graph, &ContextCapturingExecutor).await.unwrap();
        assert!(outcome.success);
        let captured = CAPTURED.lock().unwrap().take();
        assert!(captured.is_some(), "child node must receive parent context");
        let (parent_id, parent_output) = captured.unwrap();
        assert_eq!(parent_id, n1, "parent output must be from n1");
        assert_eq!(parent_output["child"], "done", "parent sees child output in graph_outputs");
    }
}
