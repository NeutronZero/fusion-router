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

use async_trait::async_trait;
use fusion_core::{NanoUSD, PlatformError};
use fusion_types::*;
use std::collections::HashSet;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub mod work_queue;
pub use work_queue::WorkQueue;

/// Per-token price in NanoUSD for a single model (review H3: the scheduler
/// previously priced EVERY node at one hardcoded flat rate, so budget
/// enforcement diverged wildly from actual provider pricing).
#[derive(Debug, Clone, Copy)]
pub struct TokenPricing {
    pub input_nanos_per_token: u64,
    pub output_nanos_per_token: u64,
}

impl TokenPricing {
    /// Historical flat fallback: $0.002/1k input, $0.01/1k output.
    pub fn flat_fallback() -> Self {
        Self {
            input_nanos_per_token: 2_000,
            output_nanos_per_token: 10_000,
        }
    }
}

/// Resolves pricing for a model name. Implementations should fall back to
/// [`TokenPricing::flat_fallback`] for unknown models rather than erroring:
/// budget enforcement must remain conservative, not fail requests.
pub type PricingResolver = std::sync::Arc<dyn Fn(&str) -> TokenPricing + Send + Sync>;

#[allow(dead_code)] // retained as documented fallback constants
const COST_PER_INPUT_TOKEN_NANOS: u64 = 2_000_000; // $0.002 per 1k tokens
#[allow(dead_code)]
const COST_PER_OUTPUT_TOKEN_NANOS: u64 = 10_000_000; // $0.01 per 1k tokens

/// Trait for executing a single node. Implementors provide the actual
/// LLM/provider dispatch. The scheduler calls this for each ready node,
/// passing a context with parent outputs.
#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute_node(
        &self,
        node: &ExecutionNode,
        ctx: &NodeExecContext,
    ) -> NodeExecutionResult;
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
    /// Optional model-aware pricing. When absent, the historical flat rate
    /// applies to every node (review H3).
    pricing: Option<PricingResolver>,
}

impl DefaultScheduler {
    pub fn new() -> Self {
        Self {
            max_concurrent: 16,
            pricing: None,
        }
    }

    pub fn with_max_concurrent(max_concurrent: usize) -> Self {
        Self {
            max_concurrent,
            pricing: None,
        }
    }

    /// Installs a model-aware pricing resolver used for cost accounting and
    /// budget-envelope enforcement.
    pub fn with_pricing(mut self, resolver: PricingResolver) -> Self {
        self.pricing = Some(resolver);
        self
    }

    fn price_for(&self, model: &str) -> TokenPricing {
        match &self.pricing {
            Some(resolve) => resolve(model),
            None => TokenPricing::flat_fallback(),
        }
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
        self.run_inner(graph, executor, Some(cancellation_token), None)
            .await
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
        self.run_inner(graph, executor, Some(cancellation_token), Some(envelope))
            .await
    }

    async fn run_inner(
        &self,
        graph: Arc<ExecutionGraph>,
        executor: &dyn Executor,
        cancel: Option<&CancellationToken>,
        envelope: Option<&BudgetEnvelope>,
    ) -> Result<ExecutionOutcome, PlatformError> {
        let mut queue =
            WorkQueue::new(graph.clone()).with_max_concurrent_nodes(self.max_concurrent);
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

        'sched_loop: loop {
            if let Some(token) = cancel {
                if token.is_cancelled() {
                    tracing::info!("Cancellation requested; aborting scheduler loop");
                    return Err(PlatformError::Scheduler {
                        code: "CANCELLED".into(),
                        message: "Request cancelled by client".into(),
                        recovery_suggestion: "Retry the request with a fresh cancellation token"
                            .into(),
                    });
                }
            }

            if let Some(env) = envelope {
                if env.max_iterations == 0 {
                    break 'sched_loop;
                }
            }

            if queue.is_done(&node_states) {
                break;
            }

            let ready = queue.get_ready(&node_states);
            if ready.is_empty() {
                if queue.any_in_progress() {
                    // Short poll while in-flight nodes finish. Cancellable so
                    // client disconnect does not wait for the full 2ms window
                    // (review M8: Notify-driven wakeup queued for v0.15).
                    if let Some(tok) = cancel {
                        tokio::select! {
                            _ = tokio::time::sleep(std::time::Duration::from_millis(2)) => {},
                            _ = tok.cancelled() => {},
                        }
                    } else {
                        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                    }
                    continue;
                }
                break;
            }

            // Collect node IDs first to release immutable borrow on queue
            let batch_ids: Vec<uuid::Uuid> = ready
                .into_iter()
                .take(self.max_concurrent)
                .map(|n| n.id)
                .collect();

            // Now we can mutate queue
            let mut handles = Vec::new();
            for &node_id in &batch_ids {
                queue.mark_in_progress(node_id);
                node_states.insert(node_id, NodeState::Running);

                // Find the node to clone for the executor — use the frozen
                // index map instead of a linear scan, and fail closed with a
                // Scheduler error rather than panicking on a desynced queue.
                let idx = match node_index.get(&node_id) {
                    Some(i) => *i,
                    None => {
                        return Err(PlatformError::Scheduler {
                            code: "NODE_NOT_FOUND".into(),
                            message: format!("scheduler queue desync: node {node_id} not in graph"),
                            recovery_suggestion: "Report this as a scheduler invariant violation".into(),
                        });
                    }
                };
                let node = graph.nodes[idx].clone();

                // Build parent context: outputs of immediate predecessors
                let incoming: Vec<uuid::Uuid> = graph
                    .edges
                    .iter()
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

            // Wait for ALL nodes in this batch concurrently — join_all polls
            // every future together, so independent nodes overlap instead of
            // running strictly serially.
            let results = futures::future::join_all(handles).await;
            for (node_id, result) in results {
                // Cost/token accounting runs for ANY result carrying usage
                // (including failed provider calls), matching the monolith —
                // then the budget envelope is checked after accumulation.
                if let Some(ref usage) = result.usage {
                    total_tokens += usage.total_tokens as u64;
                    // Model-aware per-token pricing (review H3): the node's
                    // model drives input/output rates; unknown models fall
                    // back to the conservative flat rate.
                    let model = node_index
                        .get(&node_id)
                        .and_then(|idx| graph.nodes.get(*idx))
                        .map(|n| n.model.as_str())
                        .unwrap_or("");
                    let price = self.price_for(model);
                    let input_cost = NanoUSD::from_nanos(
                        (usage.prompt_tokens as u64).saturating_mul(price.input_nanos_per_token),
                    );
                    let output_cost = NanoUSD::from_nanos(
                        (usage.completion_tokens as u64)
                            .saturating_mul(price.output_nanos_per_token),
                    );
                    let node_cost = input_cost.saturating_add(output_cost);
                    total_cost = total_cost.saturating_add(node_cost);

                    if let Some(envelope) = envelope {
                        if let Err(ref e) =
                            envelope.record_and_check(node_cost, usage.total_tokens as u64)
                        {
                            tracing::info!(node_id = ?node_id, error = %e, "Budget envelope breached; stopping further execution");

                            // Preserve the breaching node's terminal state so
                            // its completed work is not discarded.
                            match result.state {
                                NodeState::Succeeded => {
                                    node_states.insert(node_id, NodeState::Succeeded);
                                    outputs.insert(
                                        node_id,
                                        result.output.unwrap_or(serde_json::Value::Null),
                                    );
                                }
                                other_state => {
                                    node_states.insert(node_id, other_state);
                                }
                            }

                            // Siblings of this batch whose results were never
                            // processed must not linger as phantom Running.
                            for &batch_id in &batch_ids {
                                if !matches!(
                                    node_states.get(&batch_id),
                                    Some(
                                        NodeState::Succeeded
                                            | NodeState::Failed(_)
                                            | NodeState::Skipped
                                    )
                                ) {
                                    node_states.insert(
                                        batch_id,
                                        NodeState::Failed(format!("Budget breached: {e}")),
                                    );
                                }
                            }

                            for node in &graph.nodes {
                                if !node_states.contains_key(&node.id)
                                    || matches!(
                                        node_states.get(&node.id),
                                        Some(NodeState::Pending | NodeState::Running)
                                    )
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

                        let node_kind = queue
                            .graph()
                            .nodes
                            .get(node_index[&node_id])
                            .map(|n| n.kind.clone());

                        match node_kind {
                            Some(ExecutionNodeKind::Conditional) => {
                                queue.mark_conditional_completed(node_id);
                                let result_val = outputs
                                    .get(&node_id)
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("true");
                                let matched_targets: Vec<uuid::Uuid> = queue
                                    .outgoing_edges(node_id)
                                    .iter()
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
                                let should_continue = outputs
                                    .get(&node_id)
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                if should_continue {
                                    // Budget iteration cap is enforced at the
                                    // loop head re-arm only (not per scheduler
                                    // wave) so deep non-loop DAGs are never
                                    // truncated early (review M10).
                                    if let Some(envelope) = envelope {
                                        if envelope.increment_iteration().is_err() {
                                            tracing::info!(
                                                "Budget iteration limit reached; stopping scheduler loop"
                                            );
                                            break 'sched_loop;
                                        }
                                    }
                                    let body_ids: Vec<uuid::Uuid> = queue
                                        .outgoing_edges(node_id)
                                        .iter()
                                        .filter(|e| e.condition.as_deref() != Some("exit"))
                                        .map(|e| e.to)
                                        .collect();
                                    for &body_id in &body_ids {
                                        node_states.insert(body_id, NodeState::Pending);
                                    }
                                    queue.reset_loop_body(&body_ids);
                                    // Loop sources never blanket-activate
                                    // downstream; re-arm only the body edges.
                                    // The `"exit"` targets stay gated until a
                                    // non-continue decision below.
                                    for &body_id in &body_ids {
                                        queue.activate_edge(node_id, body_id);
                                    }
                                } else {
                                    let exit_targets: Vec<uuid::Uuid> = queue
                                        .outgoing_edges(node_id)
                                        .iter()
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
                                        let iter_count =
                                            loop_iterations.entry(loop_node_id).or_insert(0);
                                        let max_iters = queue.graph().nodes
                                            [node_index[&loop_node_id]]
                                            .config
                                            .get("max_iterations")
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(10)
                                            as u32;
                                        if *iter_count < max_iters {
                                            *iter_count += 1;
                                            tracing::info!(
                                                loop_node_id = ?loop_node_id,
                                                iteration = *iter_count,
                                                max = max_iters,
                                                "Loop iteration"
                                            );
                                            let body_ids: Vec<uuid::Uuid> = queue
                                                .outgoing_edges(loop_node_id)
                                                .iter()
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
                        node_states.insert(node_id, NodeState::Failed(msg.clone()));
                        // Cascade: mark all downstream dependents as Failed so
                        // they are never left in an ambiguous Pending state.
                        let mut stack: Vec<uuid::Uuid> = queue
                            .outgoing_edges(node_id)
                            .iter()
                            .map(|e| e.to)
                            .collect();
                        let mut visited = HashSet::new();
                        while let Some(downstream) = stack.pop() {
                            if !visited.insert(downstream) {
                                continue;
                            }
                            if matches!(
                                node_states.get(&downstream),
                                Some(NodeState::Pending) | None
                            ) {
                                node_states.insert(
                                    downstream,
                                    NodeState::Failed("dependency failed".into()),
                                );
                                queue.mark_failed(downstream);
                                let edges: Vec<uuid::Uuid> = queue
                                    .outgoing_edges(downstream)
                                    .iter()
                                    .map(|e| e.to)
                                    .collect();
                                stack.extend(edges);
                            }
                        }
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
        async fn execute_node(
            &self,
            _node: &ExecutionNode,
            _ctx: &NodeExecContext,
        ) -> NodeExecutionResult {
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
                retry_policy: RetryPolicy {
                    max_retries: 0,
                    backoff_ms: 0,
                },
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
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: n2,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "m2".into(),
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
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
            async fn execute_node(
                &self,
                node: &ExecutionNode,
                _ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
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
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: n2,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "ok".into(),
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
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
        // NOTE: static is safe here because this is the only test using this
        // pattern. If more tests need executor-to-test communication, switch
        // to Arc<parking_lot::Mutex<>> passed through the executor struct.
        static CAPTURED: Mutex<Option<(uuid::Uuid, serde_json::Value)>> = Mutex::new(None);

        struct ContextCapturingExecutor;
        #[async_trait]
        impl Executor for ContextCapturingExecutor {
            async fn execute_node(
                &self,
                node: &ExecutionNode,
                ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
                if node.model == "child" {
                    let parent = ctx
                        .parent_outputs
                        .iter()
                        .next()
                        .map(|(k, v)| (*k, v.clone()));
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
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
                    fallback: None,
                    config: HashMap::new(),
                    subgraph: None,
                },
                ExecutionNode {
                    id: n2,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: "child".into(),
                    retry_policy: RetryPolicy {
                        max_retries: 0,
                        backoff_ms: 0,
                    },
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
        let outcome = scheduler
            .run(graph, &ContextCapturingExecutor)
            .await
            .unwrap();
        assert!(outcome.success);
        let captured = CAPTURED.lock().unwrap().take();
        assert!(captured.is_some(), "child node must receive parent context");
        let (parent_id, parent_output) = captured.unwrap();
        assert_eq!(parent_id, n1, "parent output must be from n1");
        assert_eq!(
            parent_output["child"], "done",
            "parent sees child output in graph_outputs"
        );
    }

    // -----------------------------------------------------------------------
    // Phase 6.3 parity: monolith `run_inner` semantics
    // -----------------------------------------------------------------------

    struct RecordingExecutor {
        executed: std::sync::Mutex<Vec<String>>,
    }

    impl RecordingExecutor {
        fn new() -> Self {
            Self {
                executed: std::sync::Mutex::new(Vec::new()),
            }
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
                retry_policy: RetryPolicy {
                    max_retries: 0,
                    backoff_ms: 0,
                },
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
            async fn execute_node(
                &self,
                node: &ExecutionNode,
                _ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
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
                ExecutionEdge {
                    from: cond,
                    to: yes,
                    condition: Some("allow".into()),
                },
                ExecutionEdge {
                    from: cond,
                    to: no,
                    condition: Some("deny".into()),
                },
            ],
        );

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler
            .run(graph, &CondExecutor(recorder.clone()))
            .await
            .unwrap();
        let calls = recorder.calls();
        assert!(calls.contains(&"cond".to_string()));
        assert!(
            calls.contains(&"yes-branch".to_string()),
            "matched branch must run"
        );
        assert!(
            !calls.contains(&"no-branch".to_string()),
            "unmatched branch must not run"
        );
        // Monolith parity: an un-taken conditional branch stays Pending, so
        // `success` requires every node Succeeded/Skipped and is false here.
        assert!(
            !outcome.success,
            "un-taken branch must not complete the graph"
        );
        assert!(matches!(
            outcome.node_states.get(&yes),
            Some(NodeState::Succeeded)
        ));
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
            async fn execute_node(
                &self,
                node: &ExecutionNode,
                _ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
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
        loop_node
            .config
            .insert("max_iterations".into(), serde_json::json!(3));
        let graph = RecordingExecutor::graph_of(
            vec![
                loop_node,
                RecordingExecutor::node_node(body, ExecutionNodeKind::LLMGenerate, "body"),
                RecordingExecutor::node_node(exit, ExecutionNodeKind::LLMGenerate, "exit-node"),
            ],
            vec![
                ExecutionEdge {
                    from: loop_id,
                    to: body,
                    condition: None,
                },
                ExecutionEdge {
                    from: loop_id,
                    to: exit,
                    condition: Some("exit".into()),
                },
                ExecutionEdge {
                    from: body,
                    to: loop_id,
                    condition: Some("loop".into()),
                },
            ],
        );

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler
            .run(
                graph,
                &LoopExecutor {
                    loop_calls: loop_calls.clone(),
                    body_calls: body_calls.clone(),
                    exit_calls: exit_calls.clone(),
                },
            )
            .await
            .unwrap();
        assert!(outcome.success);
        // Monolith parity: the loop node re-runs once for the initial pass plus
        // one re-arm per loop-back iteration (`max_iterations`), for a total of
        // `max_iterations + 1` executions; the exit branch is taken once.
        assert_eq!(
            loop_calls.load(Ordering::SeqCst),
            4,
            "loop runs initial pass + 3 loop-backs"
        );
        assert_eq!(
            body_calls.load(Ordering::SeqCst),
            4,
            "body runs once per loop execution"
        );
        assert_eq!(
            exit_calls.load(Ordering::SeqCst),
            1,
            "exit branch taken once"
        );
    }

    #[tokio::test]
    async fn test_loop_back_respects_max_iterations() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let loop_calls = Arc::new(AtomicUsize::new(0));

        struct BackExecutor(Arc<AtomicUsize>);
        #[async_trait]
        impl Executor for BackExecutor {
            async fn execute_node(
                &self,
                node: &ExecutionNode,
                _ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
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
        loop_node
            .config
            .insert("max_iterations".into(), serde_json::json!(2));
        let graph = RecordingExecutor::graph_of(
            vec![
                loop_node,
                RecordingExecutor::node_node(body, ExecutionNodeKind::LLMGenerate, "body"),
            ],
            vec![
                ExecutionEdge {
                    from: loop_id,
                    to: body,
                    condition: None,
                },
                ExecutionEdge {
                    from: body,
                    to: loop_id,
                    condition: Some("loop".into()),
                },
            ],
        );

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler
            .run(graph, &BackExecutor(loop_calls.clone()))
            .await
            .unwrap();
        assert!(outcome.success);
        // Monolith parity: `max_iterations` loop-backs plus the initial pass.
        assert_eq!(
            loop_calls.load(Ordering::SeqCst),
            3,
            "loop runs initial pass + 2 loop-backs before the cap stops re-arming"
        );
    }

    #[tokio::test]
    async fn test_cancellation_before_run_errors() {
        let node = uuid::Uuid::new_v4();
        let graph = RecordingExecutor::graph_of(
            vec![RecordingExecutor::node_node(
                node,
                ExecutionNodeKind::LLMGenerate,
                "n",
            )],
            vec![],
        );
        let token = CancellationToken::new();
        token.cancel();

        let scheduler = DefaultScheduler::new();
        let err = scheduler
            .run_with_cancellation(graph, &MockExecutor, &token)
            .await;
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
            async fn execute_node(
                &self,
                _node: &ExecutionNode,
                _ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
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
        let executor = SlowExecutor {
            started: started.clone(),
        };
        let handle = tokio::spawn({
            let token = token.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                token.cancel();
            }
        });
        let outcome = scheduler
            .run_with_cancellation(graph, &executor, &token)
            .await;
        handle.await.unwrap();

        assert!(
            started.load(Ordering::SeqCst) >= 1,
            "slow node must have started"
        );
        match outcome {
            Err(PlatformError::Scheduler { code, .. }) => assert_eq!(code, "CANCELLED"),
            Err(other) => panic!("unexpected error: {other:?}"),
            Ok(outcome) => {
                assert!(!outcome.success, "cancelled nodes must not yield success");
                assert!(
                    outcome
                        .node_states
                        .values()
                        .any(|s| matches!(s, NodeState::Failed(m) if m == "Cancelled by client")),
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
            async fn execute_node(
                &self,
                _node: &ExecutionNode,
                _ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
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
            vec![RecordingExecutor::node_node(
                node,
                ExecutionNodeKind::LLMGenerate,
                "n",
            )],
            vec![],
        );

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler.run(graph, &PricingExecutor).await.unwrap();
        let expected = NanoUSD::from_nanos(
            1000 * TokenPricing::flat_fallback().input_nanos_per_token
                + 500 * TokenPricing::flat_fallback().output_nanos_per_token,
        );
        assert_eq!(
            outcome.total_cost, expected,
            "cost must use per-token rates: {:?} vs {:?}",
            outcome.total_cost, expected
        );
        assert_eq!(outcome.total_tokens, 1500);
    }

    #[tokio::test]
    async fn test_budget_iteration_cap_stops_run() {
        let node = uuid::Uuid::new_v4();
        let graph = RecordingExecutor::graph_of(
            vec![RecordingExecutor::node_node(
                node,
                ExecutionNodeKind::LLMGenerate,
                "n",
            )],
            vec![],
        );
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 1000, 0);

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler
            .run_with_budget(graph, &MockExecutor, &env)
            .await
            .unwrap();

        // Monolith parity: iteration cap breached at the first loop head,
        // before any node runs; the graph completes unsuccessfully.
        assert!(!outcome.success);
        assert!(
            !outcome.node_states.contains_key(&node),
            "node must never run under a zero-iteration envelope"
        );
    }

    #[tokio::test]
    async fn test_budget_breach_skips_pending_nodes_and_counts_usage() {
        struct UsageExecutor;
        #[async_trait]
        impl Executor for UsageExecutor {
            async fn execute_node(
                &self,
                _node: &ExecutionNode,
                _ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
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
            vec![ExecutionEdge {
                from: n1,
                to: n2,
                condition: None,
            }],
        );
        // Token limit 5 < 20 consumed per node: breach on the first usage.
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 5, 10);

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler
            .run_with_budget(graph, &UsageExecutor, &env)
            .await
            .unwrap();

        assert!(
            !outcome.success,
            "breach must complete the run unsuccessfully"
        );
        assert_eq!(
            outcome.total_tokens, 20,
            "usage from the succeeded node must still be counted"
        );
        assert_eq!(
            outcome.total_cost,
            NanoUSD::from_nanos(
                10 * TokenPricing::flat_fallback().input_nanos_per_token
                    + 10 * TokenPricing::flat_fallback().output_nanos_per_token
            )
        );
        assert!(
            matches!(outcome.node_states.get(&n2), Some(NodeState::Skipped)),
            "outstanding node must be skipped"
        );
    }

    // -----------------------------------------------------------------------
    // Concurrency: batch members must overlap, not await serially
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_independent_batch_nodes_execute_concurrently() {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};

        struct SleepExecutor {
            starts: Mutex<Vec<Instant>>,
        }

        #[async_trait]
        impl Executor for SleepExecutor {
            async fn execute_node(
                &self,
                _node: &ExecutionNode,
                _ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
                self.starts.lock().unwrap().push(Instant::now());
                tokio::time::sleep(Duration::from_millis(50)).await;
                NodeExecutionResult {
                    state: NodeState::Succeeded,
                    usage: None,
                    latency_ms: 50,
                    output: Some(serde_json::json!("slept")),
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

        let executor = SleepExecutor {
            starts: Mutex::new(Vec::new()),
        };
        let scheduler = DefaultScheduler::new();
        let start = Instant::now();
        let outcome = scheduler.run(graph, &executor).await.unwrap();
        let elapsed = start.elapsed();

        assert!(outcome.success);
        let starts = executor.starts.lock().unwrap().clone();
        assert_eq!(starts.len(), 2, "both nodes must have executed");
        let spread = starts
            .iter()
            .max()
            .unwrap()
            .duration_since(*starts.iter().min().unwrap());
        assert!(
            spread < Duration::from_millis(40),
            "batch nodes must start together, spread was {spread:?}"
        );
        assert!(
            elapsed < Duration::from_millis(95),
            "two 50ms nodes awaited serially would exceed 100ms; took {elapsed:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Budget breach mid-batch: no discarded results, no phantom Running
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_budget_breach_records_result_and_fails_unprocessed_batch_members() {
        struct UsageExecutor;

        #[async_trait]
        impl Executor for UsageExecutor {
            async fn execute_node(
                &self,
                _node: &ExecutionNode,
                _ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
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
            vec![],
        );
        // Token limit 5 < 20 consumed per node: the first processed result
        // breaches while its sibling is still unresolved in the same batch.
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(10_000_000), 5, 100);

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler
            .run_with_budget(graph, &UsageExecutor, &env)
            .await
            .unwrap();

        assert!(!outcome.success);
        assert_eq!(
            outcome.total_tokens, 20,
            "only results processed before the breach are counted"
        );
        assert!(
            matches!(outcome.node_states.get(&n1), Some(NodeState::Succeeded)),
            "the breaching node's succeeded work must be recorded, not discarded"
        );
        assert_eq!(
            outcome.outputs.get(&n1),
            Some(&serde_json::json!("ok")),
            "the breaching node's output must survive the early return"
        );
        match outcome.node_states.get(&n2) {
            Some(NodeState::Failed(msg)) => assert!(
                msg.contains("Budget"),
                "unresolved sibling must fail with a budget-breach reason, got: {msg}"
            ),
            other => panic!("sibling must be terminally Failed, got {other:?}"),
        }
        assert!(
            outcome
                .node_states
                .values()
                .all(|s| !matches!(s, NodeState::Running)),
            "no phantom Running states may remain: {:?}",
            outcome.node_states
        );
    }

    // -----------------------------------------------------------------------
    // Loop gating: should_continue decides body-vs-exit activation
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_loop_exit_target_runs_only_after_loop_finishes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Mutex;
        let loop_calls = Arc::new(AtomicUsize::new(0));
        let body_calls = Arc::new(AtomicUsize::new(0));
        let exit_calls = Arc::new(AtomicUsize::new(0));
        let order: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        struct GatedLoopExecutor {
            loop_calls: Arc<AtomicUsize>,
            body_calls: Arc<AtomicUsize>,
            exit_calls: Arc<AtomicUsize>,
            order: Arc<Mutex<Vec<String>>>,
        }
        #[async_trait]
        impl Executor for GatedLoopExecutor {
            async fn execute_node(
                &self,
                node: &ExecutionNode,
                _ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
                self.order.lock().unwrap().push(node.model.clone());
                if node.model == "loop" {
                    let n = self.loop_calls.fetch_add(1, Ordering::SeqCst) + 1;
                    let out = serde_json::json!(n <= 3);
                    NodeExecutionResult {
                        state: NodeState::Succeeded,
                        usage: None,
                        latency_ms: 0,
                        output: Some(out),
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
        loop_node
            .config
            .insert("max_iterations".into(), serde_json::json!(3));
        let graph = RecordingExecutor::graph_of(
            vec![
                loop_node,
                RecordingExecutor::node_node(body, ExecutionNodeKind::LLMGenerate, "body"),
                RecordingExecutor::node_node(exit, ExecutionNodeKind::LLMGenerate, "exit-node"),
            ],
            vec![
                ExecutionEdge {
                    from: loop_id,
                    to: body,
                    condition: None,
                },
                ExecutionEdge {
                    from: loop_id,
                    to: exit,
                    condition: Some("exit".into()),
                },
                ExecutionEdge {
                    from: body,
                    to: loop_id,
                    condition: Some("loop".into()),
                },
            ],
        );

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler
            .run(
                graph,
                &GatedLoopExecutor {
                    loop_calls: loop_calls.clone(),
                    body_calls: body_calls.clone(),
                    exit_calls: exit_calls.clone(),
                    order: order.clone(),
                },
            )
            .await
            .unwrap();

        assert!(outcome.success);
        assert_eq!(
            loop_calls.load(Ordering::SeqCst),
            4,
            "parity: initial pass + max_iterations loop-backs"
        );
        assert_eq!(body_calls.load(Ordering::SeqCst), 4);
        assert_eq!(exit_calls.load(Ordering::SeqCst), 1);
        let order = order.lock().unwrap().clone();
        assert_eq!(
            order.last().map(String::as_str),
            Some("exit-node"),
            "exit target must execute only after all loop/body iterations, order: {order:?}"
        );
        assert_eq!(
            order.iter().filter(|m| m.as_str() == "exit-node").count(),
            1,
            "exit target must execute exactly once"
        );
    }

    #[tokio::test]
    async fn test_loop_always_continue_never_activates_exit_target() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let loop_calls = Arc::new(AtomicUsize::new(0));
        let exit_calls = Arc::new(AtomicUsize::new(0));

        struct AlwaysContinueExecutor {
            loop_calls: Arc<AtomicUsize>,
            exit_calls: Arc<AtomicUsize>,
        }
        #[async_trait]
        impl Executor for AlwaysContinueExecutor {
            async fn execute_node(
                &self,
                node: &ExecutionNode,
                _ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
                if node.model == "loop" {
                    self.loop_calls.fetch_add(1, Ordering::SeqCst);
                    NodeExecutionResult {
                        state: NodeState::Succeeded,
                        usage: None,
                        latency_ms: 0,
                        output: Some(serde_json::json!(true)),
                    }
                } else if node.model == "body" {
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
                        output: Some(serde_json::json!("premature-exit")),
                    }
                }
            }
        }

        let loop_id = uuid::Uuid::new_v4();
        let body = uuid::Uuid::new_v4();
        let exit = uuid::Uuid::new_v4();
        let mut loop_node = RecordingExecutor::node_node(loop_id, ExecutionNodeKind::Loop, "loop");
        loop_node
            .config
            .insert("max_iterations".into(), serde_json::json!(2));
        let graph = RecordingExecutor::graph_of(
            vec![
                loop_node,
                RecordingExecutor::node_node(body, ExecutionNodeKind::LLMGenerate, "body"),
                RecordingExecutor::node_node(exit, ExecutionNodeKind::LLMGenerate, "exit-node"),
            ],
            vec![
                ExecutionEdge {
                    from: loop_id,
                    to: body,
                    condition: None,
                },
                ExecutionEdge {
                    from: loop_id,
                    to: exit,
                    condition: Some("exit".into()),
                },
                ExecutionEdge {
                    from: body,
                    to: loop_id,
                    condition: Some("loop".into()),
                },
            ],
        );

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler
            .run(
                graph,
                &AlwaysContinueExecutor {
                    loop_calls: loop_calls.clone(),
                    exit_calls: exit_calls.clone(),
                },
            )
            .await
            .unwrap();

        assert_eq!(
            exit_calls.load(Ordering::SeqCst),
            0,
            "exit-conditioned edge must never activate while should_continue is true"
        );
        assert_eq!(
            loop_calls.load(Ordering::SeqCst),
            3,
            "parity: initial pass + max_iterations loop-backs terminate the loop"
        );
        assert!(
            !matches!(outcome.node_states.get(&exit), Some(NodeState::Succeeded)),
            "exit node must remain untouched (Pending)"
        );
        assert!(
            !outcome.success,
            "an un-activated exit target keeps the graph incomplete, like un-taken conditional branches"
        );
    }

    #[tokio::test]
    async fn test_loop_head_waits_for_upstream_dependency() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::Mutex;
        use std::time::Duration;
        let loop_calls = Arc::new(AtomicUsize::new(0));
        let saw_dependency = Arc::new(AtomicBool::new(false));
        let order: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        struct DependentLoopExecutor {
            dep_id: uuid::Uuid,
            loop_calls: Arc<AtomicUsize>,
            saw_dependency: Arc<AtomicBool>,
            order: Arc<Mutex<Vec<String>>>,
        }
        #[async_trait]
        impl Executor for DependentLoopExecutor {
            async fn execute_node(
                &self,
                node: &ExecutionNode,
                ctx: &NodeExecContext,
            ) -> NodeExecutionResult {
                self.order.lock().unwrap().push(node.model.clone());
                if node.model == "dep" {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    return NodeExecutionResult {
                        state: NodeState::Succeeded,
                        usage: None,
                        latency_ms: 50,
                        output: Some(serde_json::json!("dep-done")),
                    };
                }
                self.loop_calls.fetch_add(1, Ordering::SeqCst);
                if ctx.parent_outputs.contains_key(&self.dep_id) {
                    self.saw_dependency.store(true, Ordering::SeqCst);
                }
                NodeExecutionResult {
                    state: NodeState::Succeeded,
                    usage: None,
                    latency_ms: 0,
                    output: Some(serde_json::json!(false)),
                }
            }
        }

        let dep = uuid::Uuid::new_v4();
        let loop_id = uuid::Uuid::new_v4();
        let graph = RecordingExecutor::graph_of(
            vec![
                RecordingExecutor::node_node(dep, ExecutionNodeKind::LLMGenerate, "dep"),
                RecordingExecutor::node_node(loop_id, ExecutionNodeKind::Loop, "loop"),
            ],
            vec![ExecutionEdge {
                from: dep,
                to: loop_id,
                condition: None,
            }],
        );

        let scheduler = DefaultScheduler::new();
        let outcome = scheduler
            .run(
                graph,
                &DependentLoopExecutor {
                    dep_id: dep,
                    loop_calls: loop_calls.clone(),
                    saw_dependency: saw_dependency.clone(),
                    order: order.clone(),
                },
            )
            .await
            .unwrap();

        assert!(outcome.success);
        let order = order.lock().unwrap().clone();
        assert_eq!(
            order.first().map(String::as_str),
            Some("dep"),
            "dependency must execute before the loop head becomes ready, order: {order:?}"
        );
        assert_eq!(loop_calls.load(Ordering::SeqCst), 1);
        assert!(
            saw_dependency.load(Ordering::SeqCst),
            "loop head must run only after its upstream dependency and receive its output"
        );
    }
}
