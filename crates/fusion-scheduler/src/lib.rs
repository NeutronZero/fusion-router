//! DAG scheduler with WorkQueue for topological execution.
//!
//! The `WorkQueue` maintains execution state and provides ready-node selection.
//! The `DefaultScheduler` runs a DAG to completion using an `Executor` trait.
//!
//! Phase 6.3 parity: the execution loop is a port of the monolith's
//! `src/scheduler/default.rs::run_inner` semantic contract —
//! cancellation (loop-head check + per-node `select!`), `Conditional` edge
//! activation from node output, `Loop` continue/exit (`"exit"`-conditioned
//! edges), loop-back iteration caps (`max_iterations` on the loop node),
//! per-token cost accounting, and optional `BudgetEnvelope` enforcement
//! (iteration cap at the loop head; cost/token checks after each node's
//! usage — a breach skips outstanding nodes and completes the run as
//! unsuccessful). Retry/fallback stay in the executor
//! (`fusion_runtime::ProviderExecutor`; the src boundary adapter), not the
//! scheduler.

use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use fusion_types::*;
use fusion_core::{PlatformError, NanoUSD};
use tokio_util::sync::CancellationToken;

pub mod work_queue;
pub use work_queue::WorkQueue;

const COST_PER_INPUT_TOKEN_NANOS: u64 = 2_000_000; // $0.002 per 1k tokens = 2M NanoUSD per 1k tokens
const COST_PER_OUTPUT_TOKEN_NANOS: u64 = 10_000_000; // $0.01 per 1k tokens = 10M NanoUSD per 1k tokens

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
    pub node_states: HashMap<uuid::Uuid, NodeState>,
    pub total_latency_ms: u64,
    pub total_cost: NanoUSD,
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
        self.run_inner(graph, executor, None, None).await
    }

    /// Execute a graph under a per-request budget envelope.
    /// `max_iterations` bounds outer loop iterations (guards runaway graphs);
    /// cost/token limits are checked after each node's usage, skipping all
    /// outstanding nodes and completing unsuccessfully on breach.
    pub async fn run_with_budget(
        &self,
        graph: Arc<ExecutionGraph>,
        executor: &dyn Executor,
        envelope: &BudgetEnvelope,
    ) -> Result<ExecutionOutcome, PlatformError> {
        self.run_inner(graph, executor, None, Some(envelope)).await
    }

    /// Execute a graph with client cancellation. The loop checks the token
    /// before each batch and races every node execution against it (biased);
    /// a cancelled node produces `Failed("Cancelled by client")`, and a
    /// cancelled loop-head returns `PlatformError::Scheduler`.
    pub async fn run_with_cancellation(
        &self,
        graph: Arc<ExecutionGraph>,
        executor: &dyn Executor,
        cancellation_token: &CancellationToken,
    ) -> Result<ExecutionOutcome, PlatformError> {
        self.run_inner(graph, executor, Some(cancellation_token), None).await
    }

    /// Execute a graph with client cancellation AND a per-request budget
    /// envelope (see `run_with_budget` for the enforcement contract).
    pub async fn run_with_cancellation_and_budget(
        &self,
        graph: Arc<ExecutionGraph>,
        executor: &dyn Executor,
        cancellation_token: &CancellationToken,
        envelope: &BudgetEnvelope,
    ) -> Result<ExecutionOutcome, PlatformError> {
        self.run_inner(graph, executor, Some(cancellation_token), Some(envelope)).await
    }

    async fn run_inner(
        &self,
        graph: Arc<ExecutionGraph>,
        executor: &dyn Executor,
        cancel: Option<&CancellationToken>,
        envelope: Option<&BudgetEnvelope>,
    ) -> Result<ExecutionOutcome, PlatformError> {
        let mut queue = WorkQueue::new(graph.clone())
            .with_max_concurrent_nodes(self.max_concurrent);
        let mut node_states: HashMap<uuid::Uuid, NodeState> = HashMap::new();
        let mut outputs: HashMap<uuid::Uuid, serde_json::Value> = HashMap::new();
        let start = std::time::Instant::now();
        let mut total_cost: NanoUSD = NanoUSD::ZERO;
        let mut total_tokens: u64 = 0;
        let mut loop_iterations: HashMap<uuid::Uuid, u32> = HashMap::new();

        // Frozen graph: node positions never change, so an id -> index map
        // turns repeated linear scans into O(1) lookups.
        let node_index: HashMap<uuid::Uuid, usize> = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();

        loop {
            if let Some(token) = cancel {
                if token.is_cancelled() {
                    tracing::info!("Cancellation requested; aborting scheduler loop");
                    return Err(PlatformError::Scheduler {
                        code: "CANCELLED".into(),
                        message: "Request cancelled by client".into(),
                        recovery_suggestion: "Retry the request with a fresh cancellation token".into(),
                    });
                }
            }

            // Enforce per-request budget iteration limit (same position as the
            // monolith run_inner: loop head, after the cancellation check).
            if let Some(envelope) = envelope {
                if envelope.increment_iteration().is_err() {
                    tracing::info!("Budget iteration limit reached; stopping scheduler loop");
                    break;
                }
            }

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
                let cancel_token = cancel.cloned();
                handles.push(async move {
                    if let Some(token) = cancel_token {
                        tokio::select! {
                            biased;
                            _ = token.cancelled() => {
                                (node.id, NodeExecutionResult {
                                    state: NodeState::Failed("Cancelled by client".into()),
                                    usage: None,
                                    latency_ms: 0,
                                    output: None,
                                })
                            }
                            result = executor_ref.execute_node(&node, &ctx) => {
                                (node.id, result)
                            }
                        }
                    } else {
                        (node.id, executor_ref.execute_node(&node, &ctx).await)
                    }
                });
            }

            // Wait for all nodes in this batch
            for handle in handles {
                let (node_id, result) = handle.await;

                // Cost/token accounting runs for ANY result carrying usage
                // (including failed provider calls), matching the monolith —
                // then the budget envelope is checked after accumulation.
                if let Some(ref usage) = result.usage {
                    total_tokens += usage.total_tokens as u64;
                    // NanoUSD: cost per token in nanos = nanos_per_1k / 1000
                    let input_cost = NanoUSD::from_nanos(
                        (usage.prompt_tokens as u64).saturating_mul(COST_PER_INPUT_TOKEN_NANOS / 1000)
                    );
                    let output_cost = NanoUSD::from_nanos(
                        (usage.completion_tokens as u64).saturating_mul(COST_PER_OUTPUT_TOKEN_NANOS / 1000)
                    );
                    let node_cost = input_cost.saturating_add(output_cost);
                    total_cost = total_cost.saturating_add(node_cost);

                    if let Some(envelope) = envelope {
                        if let Err(ref e) = envelope.record_and_check(node_cost, usage.total_tokens as u64) {
                            tracing::info!(node_id = ?node_id, error = %e, "Budget envelope breached; stopping further execution");
                            for node in &graph.nodes {
                                if !node_states.contains_key(&node.id)
                                    || matches!(node_states.get(&node.id), Some(NodeState::Pending))
                                {
                                    node_states.insert(node.id, NodeState::Skipped);
                                }
                            }
                            let total_latency_ms = start.elapsed().as_millis() as u64;
                            return Ok(ExecutionOutcome {
                                success: false,
                                outputs,
                                node_states,
                                total_latency_ms,
                                total_cost,
                                total_tokens,
                            });
                        }
                    }
                }

                match result.state {
                    NodeState::Succeeded => {
                        tracing::info!(node_id = ?node_id, latency_ms = result.latency_ms, "Node succeeded");
                        node_states.insert(node_id, NodeState::Succeeded);
                        let output_val = result.output.unwrap_or(serde_json::Value::Null);
                        outputs.insert(node_id, output_val.clone());

                        let node_kind = queue.graph().nodes.get(node_index[&node_id]).map(|n| n.kind.clone());

                        match node_kind {
                            Some(ExecutionNodeKind::Conditional) => {
                                queue.mark_conditional_completed(node_id);
                                let result_val = outputs.get(&node_id)
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("true");
                                let matched_targets: Vec<uuid::Uuid> = queue.outgoing_edges(node_id).iter()
                                    .filter(|e| match e.condition.as_deref() {
                                        Some(cond) => cond == result_val,
                                        None => true,
                                    })
                                    .map(|e| e.to)
                                    .collect();
                                for to in matched_targets {
                                    queue.activate_edge(node_id, to);
                                }
                            }
                            Some(ExecutionNodeKind::Loop) => {
                                queue.mark_completed(node_id);
                                let should_continue = outputs.get(&node_id)
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                if should_continue {
                                    let body_ids: Vec<uuid::Uuid> = queue.outgoing_edges(node_id).iter()
                                        .filter(|e| e.condition.as_deref() != Some("exit"))
                                        .map(|e| e.to)
                                        .collect();
                                    for &body_id in &body_ids {
                                        node_states.insert(body_id, NodeState::Pending);
                                    }
                                    queue.reset_loop_body(&body_ids);
                                } else {
                                    let exit_targets: Vec<uuid::Uuid> = queue.outgoing_edges(node_id).iter()
                                        .filter(|e| e.condition.as_deref() == Some("exit"))
                                        .map(|e| e.to)
                                        .collect();
                                    for to in exit_targets {
                                        queue.activate_edge(node_id, to);
                                    }
                                }
                            }
                            _ => {
                                queue.mark_completed(node_id);
                                if queue.has_loop_back_edge(node_id) {
                                    if let Some(loop_node_id) = queue.loop_back_target(node_id) {
                                        let iter_count = loop_iterations.entry(loop_node_id).or_insert(0);
                                        let max_iters = queue.graph().nodes[node_index[&loop_node_id]]
                                            .config
                                            .get("max_iterations")
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(10) as u32;
                                        if *iter_count < max_iters {
                                            *iter_count += 1;
                                            tracing::info!(
                                                loop_node_id = ?loop_node_id,
                                                iteration = *iter_count,
                                                max = max_iters,
                                                "Loop iteration"
                                            );
                                            let body_ids: Vec<uuid::Uuid> = queue.outgoing_edges(loop_node_id).iter()
                                                .filter(|e| e.condition.as_deref() != Some("exit"))
                                                .map(|e| e.to)
                                                .collect();
                                            for &body_id in &body_ids {
                                                node_states.insert(body_id, NodeState::Pending);
                                            }
                                            queue.reset_loop_body(&body_ids);
                                            node_states.insert(loop_node_id, NodeState::Pending);
                                            queue.reset_ready(loop_node_id);
                                            queue.activate_edge(node_id, loop_node_id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    NodeState::Failed(msg) => {
                        tracing::info!(node_id = ?node_id, reason = %msg, latency_ms = result.latency_ms, "Node failed");
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
            node_states,
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

/// Execution lease management (Invariant 12), preserved from the retired
/// fusion-placement crate.
pub mod leases;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distributed_scheduler_creates_execution_plan() {
        // Removed with the placement shim; leases module carries the
        // preserved invariant-13 coverage. See src/leases.rs tests.
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
                estimated_cost: NanoUSD::from_nanos(10_000_000),
                estimated_tokens: 150,
                policy_version: 0,
                max_depth: 1,
                node_count: 1,
            },
            total_tokens: 150,
            total_cost: NanoUSD::from_nanos(10),
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
                estimated_cost: NanoUSD::from_nanos(20_000_000),
                estimated_tokens: 300,
                policy_version: 0,
                max_depth: 2,
                node_count: 2,
            },
            total_tokens: 300,
            total_cost: NanoUSD::from_nanos(20),
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
                estimated_cost: NanoUSD::from_nanos(20_000_000),
                estimated_tokens: 300,
                policy_version: 0,
                max_depth: 2,
                node_count: 2,
            },
            total_tokens: 300,
            total_cost: NanoUSD::from_nanos(20),
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
                estimated_cost: NanoUSD::ZERO,
                estimated_tokens: 0,
                policy_version: 0,
                max_depth: 2,
                node_count: 2,
            },
            total_tokens: 0,
            total_cost: NanoUSD::ZERO,
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

    // -----------------------------------------------------------------------
    // Phase 6.3 parity: monolith `run_inner` semantics
    // -----------------------------------------------------------------------

    struct RecordingExecutor {
        executed: std::sync::Mutex<Vec<String>>,
    }

    impl RecordingExecutor {
        fn new() -> Self {
            Self { executed: std::sync::Mutex::new(Vec::new()) }
        }

        fn calls(&self) -> Vec<String> {
            self.executed.lock().unwrap().clone()
        }
    }

    impl RecordingExecutor {
        fn node_node(id: uuid::Uuid, kind: ExecutionNodeKind, model: &str) -> ExecutionNode {
            ExecutionNode {
                id,
                kind,
                strategy: StrategyKind::Single,
                model: model.into(),
                retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                fallback: None,
                config: HashMap::new(),
                subgraph: None,
            }
        }

        fn graph_of(nodes: Vec<ExecutionNode>, edges: Vec<ExecutionEdge>) -> Arc<ExecutionGraph> {
            Arc::new(ExecutionGraph {
                graph_id: uuid::Uuid::new_v4(),
                nodes,
                edges,
                metadata: GraphMetadata {
                    estimated_cost: NanoUSD::ZERO,
                    estimated_tokens: 0,
                    policy_version: 0,
                    max_depth: 4,
                    node_count: 0,
                },
                total_tokens: 0,
                total_cost: NanoUSD::ZERO,
                primitive_graph_hash: 0,
            })
        }
    }

    #[tokio::test]
    async fn test_conditional_activates_only_matching_edge() {
        struct CondExecutor(Arc<RecordingExecutor>);
        #[async_trait]
        impl Executor for CondExecutor {
            async fn execute_node(&self, node: &ExecutionNode, _ctx: &NodeExecContext) -> NodeExecutionResult {
                let out = if node.model == "cond" {
                    serde_json::json!("allow")
                } else {
                    serde_json::json!("done")
                };
                self.0.executed.lock().unwrap().push(node.model.clone());
                NodeExecutionResult {
                    state: NodeState::Succeeded,
                    usage: None,
                    latency_ms: 1,
                    output: Some(out),
                }
            }
        }

        let cond = uuid::Uuid::new_v4();
        let yes = uuid::Uuid::new_v4();
        let no = uuid::Uuid::new_v4();
        let recorder = Arc::new(RecordingExecutor::new());
        let graph = RecordingExecutor::graph_of(
            vec![
                RecordingExecutor::node_node(cond, ExecutionNodeKind::Conditional, "cond"),
                RecordingExecutor::node_node(yes, ExecutionNodeKind::LLMGenerate, "yes-branch"),
                RecordingExecutor::node_node(no, ExecutionNodeKind::LLMGenerate, "no-branch"),
            ],
            vec![
                ExecutionEdge { from: cond, to: yes, condition: Some("allow".into()) },
                ExecutionEdge { from: cond, to: no, condition: Some("deny".into()) },
            ],
        );

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler.run(graph, &CondExecutor(recorder.clone())).await.unwrap();
        let calls = recorder.calls();
        assert!(calls.contains(&"cond".to_string()));
        assert!(calls.contains(&"yes-branch".to_string()), "matched branch must run");
        assert!(!calls.contains(&"no-branch".to_string()), "unmatched branch must not run");
        // Monolith parity: an un-taken conditional branch stays Pending, so
        // `success` requires every node Succeeded/Skipped and is false here.
        assert!(!outcome.success, "un-taken branch must not complete the graph");
        assert!(matches!(outcome.node_states.get(&yes), Some(NodeState::Succeeded)));
    }

    #[tokio::test]
    async fn test_loop_continue_then_exit() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let loop_calls = Arc::new(AtomicUsize::new(0));
        let body_calls = Arc::new(AtomicUsize::new(0));
        let exit_calls = Arc::new(AtomicUsize::new(0));

        struct LoopExecutor {
            loop_calls: Arc<AtomicUsize>,
            body_calls: Arc<AtomicUsize>,
            exit_calls: Arc<AtomicUsize>,
        }
        #[async_trait]
        impl Executor for LoopExecutor {
            async fn execute_node(&self, node: &ExecutionNode, _ctx: &NodeExecContext) -> NodeExecutionResult {
                if node.model == "loop" {
                    self.loop_calls.fetch_add(1, Ordering::SeqCst);
                    let first = self.loop_calls.load(Ordering::SeqCst) == 1;
                    NodeExecutionResult {
                        state: NodeState::Succeeded,
                        usage: None,
                        latency_ms: 0,
                        output: Some(serde_json::json!(first)),
                    }
                } else if node.model == "body" {
                    self.body_calls.fetch_add(1, Ordering::SeqCst);
                    NodeExecutionResult {
                        state: NodeState::Succeeded,
                        usage: None,
                        latency_ms: 0,
                        output: Some(serde_json::json!("iterated")),
                    }
                } else {
                    self.exit_calls.fetch_add(1, Ordering::SeqCst);
                    NodeExecutionResult {
                        state: NodeState::Succeeded,
                        usage: None,
                        latency_ms: 0,
                        output: Some(serde_json::json!("exit-reached")),
                    }
                }
            }
        }

        let loop_id = uuid::Uuid::new_v4();
        let body = uuid::Uuid::new_v4();
        let exit = uuid::Uuid::new_v4();
        let mut loop_node = RecordingExecutor::node_node(loop_id, ExecutionNodeKind::Loop, "loop");
        loop_node.config.insert("max_iterations".into(), serde_json::json!(3));
        let graph = RecordingExecutor::graph_of(
            vec![loop_node, RecordingExecutor::node_node(body, ExecutionNodeKind::LLMGenerate, "body"), RecordingExecutor::node_node(exit, ExecutionNodeKind::LLMGenerate, "exit-node")],
            vec![
                ExecutionEdge { from: loop_id, to: body, condition: None },
                ExecutionEdge { from: loop_id, to: exit, condition: Some("exit".into()) },
                ExecutionEdge { from: body, to: loop_id, condition: Some("loop".into()) },
            ],
        );

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler.run(graph, &LoopExecutor {
            loop_calls: loop_calls.clone(),
            body_calls: body_calls.clone(),
            exit_calls: exit_calls.clone(),
        }).await.unwrap();
        assert!(outcome.success);
        // Monolith parity: the loop node re-runs once for the initial pass plus
        // one re-arm per loop-back iteration (`max_iterations`), for a total of
        // `max_iterations + 1` executions; the exit branch is taken once.
        assert_eq!(loop_calls.load(Ordering::SeqCst), 4, "loop runs initial pass + 3 loop-backs");
        assert_eq!(body_calls.load(Ordering::SeqCst), 4, "body runs once per loop execution");
        assert_eq!(exit_calls.load(Ordering::SeqCst), 1, "exit branch taken once");
    }

    #[tokio::test]
    async fn test_loop_back_respects_max_iterations() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let loop_calls = Arc::new(AtomicUsize::new(0));

        struct BackExecutor(Arc<AtomicUsize>);
        #[async_trait]
        impl Executor for BackExecutor {
            async fn execute_node(&self, node: &ExecutionNode, _ctx: &NodeExecContext) -> NodeExecutionResult {
                if node.model == "body" {
                    NodeExecutionResult {
                        state: NodeState::Succeeded,
                        usage: None,
                        latency_ms: 0,
                        output: Some(serde_json::json!("always-continue")),
                    }
                } else {
                    self.0.fetch_add(1, Ordering::SeqCst);
                    NodeExecutionResult {
                        state: NodeState::Succeeded,
                        usage: None,
                        latency_ms: 0,
                        output: Some(serde_json::json!(true)),
                    }
                }
            }
        }

        let loop_id = uuid::Uuid::new_v4();
        let body = uuid::Uuid::new_v4();
        let mut loop_node = RecordingExecutor::node_node(loop_id, ExecutionNodeKind::Loop, "loop");
        loop_node.config.insert("max_iterations".into(), serde_json::json!(2));
        let graph = RecordingExecutor::graph_of(
            vec![loop_node, RecordingExecutor::node_node(body, ExecutionNodeKind::LLMGenerate, "body")],
            vec![
                ExecutionEdge { from: loop_id, to: body, condition: None },
                ExecutionEdge { from: body, to: loop_id, condition: Some("loop".into()) },
            ],
        );

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler.run(graph, &BackExecutor(loop_calls.clone())).await.unwrap();
        assert!(outcome.success);
        // Monolith parity: `max_iterations` loop-backs plus the initial pass.
        assert_eq!(loop_calls.load(Ordering::SeqCst), 3, "loop runs initial pass + 2 loop-backs before the cap stops re-arming");
    }

    #[tokio::test]
    async fn test_cancellation_before_run_errors() {
        let node = uuid::Uuid::new_v4();
        let graph = RecordingExecutor::graph_of(
            vec![RecordingExecutor::node_node(node, ExecutionNodeKind::LLMGenerate, "n")],
            vec![],
        );
        let token = CancellationToken::new();
        token.cancel();

        let scheduler = DefaultScheduler::new();
        let err = scheduler.run_with_cancellation(graph, &MockExecutor, &token).await;
        assert!(matches!(err, Err(PlatformError::Scheduler { code, .. }) if code == "CANCELLED"));
    }

    #[tokio::test]
    async fn test_cancellation_mid_run_fails_pending_nodes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;
        let started = Arc::new(AtomicUsize::new(0));

        struct SlowExecutor {
            started: Arc<AtomicUsize>,
        }
        #[async_trait]
        impl Executor for SlowExecutor {
            async fn execute_node(&self, _node: &ExecutionNode, _ctx: &NodeExecContext) -> NodeExecutionResult {
                self.started.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_secs(5)).await;
                NodeExecutionResult {
                    state: NodeState::Succeeded,
                    usage: None,
                    latency_ms: 0,
                    output: Some(serde_json::json!("late")),
                }
            }
        }

        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();
        let graph = RecordingExecutor::graph_of(
            vec![
                RecordingExecutor::node_node(n1, ExecutionNodeKind::LLMGenerate, "n1"),
                RecordingExecutor::node_node(n2, ExecutionNodeKind::LLMGenerate, "n2"),
            ],
            vec![],
        );

        let token = CancellationToken::new();
        let scheduler = DefaultScheduler::new();
        let executor = SlowExecutor { started: started.clone() };
        let handle = tokio::spawn({
            let token = token.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                token.cancel();
            }
        });
        let outcome = scheduler.run_with_cancellation(graph, &executor, &token).await;
        handle.await.unwrap();

        assert!(started.load(Ordering::SeqCst) >= 1, "slow node must have started");
        match outcome {
            Err(PlatformError::Scheduler { code, .. }) => assert_eq!(code, "CANCELLED"),
            Err(other) => panic!("unexpected error: {other:?}"),
            Ok(outcome) => {
                assert!(!outcome.success, "cancelled nodes must not yield success");
                assert!(
                    outcome.node_states.values().any(|s| matches!(s, NodeState::Failed(m) if m == "Cancelled by client")),
                    "pending in-flight nodes must be failed with the cancellation message"
                );
            }
        }
    }

    #[tokio::test]
    async fn test_cost_uses_per_token_rates() {
        struct PricingExecutor;
        #[async_trait]
        impl Executor for PricingExecutor {
            async fn execute_node(&self, _node: &ExecutionNode, _ctx: &NodeExecContext) -> NodeExecutionResult {
                NodeExecutionResult {
                    state: NodeState::Succeeded,
                    usage: Some(Usage {
                        prompt_tokens: 1000,
                        completion_tokens: 500,
                        total_tokens: 1500,
                    }),
                    latency_ms: 1,
                    output: Some(serde_json::json!("ok")),
                }
            }
        }

        let node = uuid::Uuid::new_v4();
        let graph = RecordingExecutor::graph_of(
            vec![RecordingExecutor::node_node(node, ExecutionNodeKind::LLMGenerate, "n")],
            vec![],
        );

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler.run(graph, &PricingExecutor).await.unwrap();
        let expected = NanoUSD::from_nanos(1000 * COST_PER_INPUT_TOKEN_NANOS / 1000 + 500 * COST_PER_OUTPUT_TOKEN_NANOS / 1000);
        assert_eq!(outcome.total_cost, expected, "cost must use per-token rates: {:?} vs {:?}", outcome.total_cost, expected);
        assert_eq!(outcome.total_tokens, 1500);
    }

    #[tokio::test]
    async fn test_budget_iteration_cap_stops_run() {
        let node = uuid::Uuid::new_v4();
        let graph = RecordingExecutor::graph_of(
            vec![RecordingExecutor::node_node(node, ExecutionNodeKind::LLMGenerate, "n")],
            vec![],
        );
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 1000, 0);

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler.run_with_budget(graph, &MockExecutor, &env).await.unwrap();

        // Monolith parity: iteration cap breached at the first loop head,
        // before any node runs; the graph completes unsuccessfully.
        assert!(!outcome.success);
        assert!(!outcome.node_states.contains_key(&node), "node must never run under a zero-iteration envelope");
    }

    #[tokio::test]
    async fn test_budget_breach_skips_pending_nodes_and_counts_usage() {
        struct UsageExecutor;
        #[async_trait]
        impl Executor for UsageExecutor {
            async fn execute_node(&self, _node: &ExecutionNode, _ctx: &NodeExecContext) -> NodeExecutionResult {
                NodeExecutionResult {
                    state: NodeState::Succeeded,
                    usage: Some(Usage {
                        prompt_tokens: 10,
                        completion_tokens: 10,
                        total_tokens: 20,
                    }),
                    latency_ms: 1,
                    output: Some(serde_json::json!("ok")),
                }
            }
        }

        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();
        let graph = RecordingExecutor::graph_of(
            vec![
                RecordingExecutor::node_node(n1, ExecutionNodeKind::LLMGenerate, "n1"),
                RecordingExecutor::node_node(n2, ExecutionNodeKind::LLMGenerate, "n2"),
            ],
            vec![ExecutionEdge { from: n1, to: n2, condition: None }],
        );
        // Token limit 5 < 20 consumed per node: breach on the first usage.
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 5, 10);

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler.run_with_budget(graph, &UsageExecutor, &env).await.unwrap();

        assert!(!outcome.success, "breach must complete the run unsuccessfully");
        assert_eq!(outcome.total_tokens, 20, "usage from the succeeded node must still be counted");
        assert_eq!(outcome.total_cost, NanoUSD::from_nanos(10 * COST_PER_INPUT_TOKEN_NANOS / 1000 + 10 * COST_PER_OUTPUT_TOKEN_NANOS / 1000));
        assert!(matches!(outcome.node_states.get(&n2), Some(NodeState::Skipped)), "outstanding node must be skipped");
    }
}

