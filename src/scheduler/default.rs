use std::collections::HashMap;
use std::time::Instant;
use futures::stream::{self, StreamExt};
use tokio_util::sync::CancellationToken;
use tracing::{info, info_span, Instrument};
use uuid::Uuid;

use crate::executor::Executor;
use crate::scheduler::work_queue::WorkQueue;
use crate::transport::backoff::Backoff;
use crate::types::{
    ExecutionGraph, ExecutionInstance, NodeExecutionResult, ExecutionNodeKind, ExecutionResult, NodeState,
    ReservationId, SchedulerError,
};

const COST_PER_INPUT_TOKEN: f64 = 0.002 / 1000.0;
const COST_PER_OUTPUT_TOKEN: f64 = 0.01 / 1000.0;
const DEFAULT_MAX_CONCURRENT: usize = 16;

pub struct DefaultScheduler {
    max_concurrent: usize,
}

impl Default for DefaultScheduler {
    fn default() -> Self {
        Self {
            max_concurrent: DEFAULT_MAX_CONCURRENT,
        }
    }
}

impl DefaultScheduler {
    pub fn new(max_concurrent: usize) -> Self {
        Self { max_concurrent }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_sets_max_concurrent() {
        let scheduler = DefaultScheduler::new(4);
        assert_eq!(scheduler.max_concurrent, 4);
    }

    #[test]
    fn test_default_uses_standard_concurrency() {
        let scheduler = DefaultScheduler::default();
        assert_eq!(scheduler.max_concurrent, DEFAULT_MAX_CONCURRENT);
    }
}

#[async_trait::async_trait]
impl crate::scheduler::Scheduler for DefaultScheduler {
    #[tracing::instrument(skip(self, graph), fields(node_count = graph.nodes.len()))]
    fn schedule(&self, graph: ExecutionGraph, reservation: ReservationId) -> ExecutionInstance {
        let mut node_states = HashMap::new();
        for node in &graph.nodes {
            node_states.insert(node.id, NodeState::Pending);
        }

        ExecutionInstance {
            instance_id: Uuid::new_v4(),
            graph,
            node_states,
            outputs: HashMap::new(),
            reservation_id: reservation.0,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
            terminal_node_id: None,
            final_output: None,
            budget_envelope: None,
        }
    }

    #[tracing::instrument(skip(self, instance, executor), fields(graph_id = %instance.graph.graph_id, node_count = instance.node_states.len()))]
    async fn run(
        &self,
        instance: &mut ExecutionInstance,
        executor: &dyn Executor,
    ) -> Result<ExecutionResult, SchedulerError> {
        self.run_inner(instance, executor, None).await
    }

    #[tracing::instrument(skip(self, instance, executor, cancellation_token), fields(graph_id = %instance.graph.graph_id))]
    async fn run_with_cancellation(
        &self,
        instance: &mut ExecutionInstance,
        executor: &dyn Executor,
        cancellation_token: &CancellationToken,
    ) -> Result<ExecutionResult, SchedulerError> {
        self.run_inner(instance, executor, Some(cancellation_token)).await
    }
}

impl DefaultScheduler {
    // Single authoritative execution loop.
    // All scheduler entry points MUST delegate here.
    async fn run_inner(
        &self,
        instance: &mut ExecutionInstance,
        executor: &dyn Executor,
        cancel: Option<&CancellationToken>,
    ) -> Result<ExecutionResult, SchedulerError> {
        let start = Instant::now();
        let mut total_tokens: u64 = 0;
        let mut total_cost: f64 = 0.0;
        let mut retry_counts: HashMap<Uuid, u32> = HashMap::new();
        let mut retry_backoffs: HashMap<Uuid, Backoff> = HashMap::new();
        let mut loop_iterations: HashMap<Uuid, u32> = HashMap::new();

        let mut queue = WorkQueue::new(instance.graph.clone());

        // Frozen graph: node positions never change, so an id -> index map
        // turns repeated linear scans into O(1) lookups.
        let node_index: HashMap<Uuid, usize> = queue
            .graph()
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();

        loop {
            if let Some(cancellation_token) = cancel {
                if cancellation_token.is_cancelled() {
                    info!("Cancellation requested; aborting scheduler loop");
                    return Err(SchedulerError::Internal(
                        "Request cancelled by client".into(),
                    ));
                }
            }

            // Enforce per-request budget iteration limit
            if let Some(ref envelope) = instance.budget_envelope {
                if envelope.increment_iteration().is_err() {
                    info!("Budget iteration limit reached; stopping scheduler loop");
                    break;
                }
            }

            let ready_ids: Vec<Uuid> = {
                let ready = queue.get_ready(&instance.node_states);
                if ready.is_empty() && queue.is_done(&instance.node_states) {
                    break;
                }
                if ready.is_empty() {
                    break;
                }
                ready.iter().map(|n| n.id).collect()
            };

            for &id in &ready_ids {
                queue.mark_in_progress(id);
                instance.node_states.insert(id, NodeState::Running);
            }

            let node_clones: Vec<_> = ready_ids
                .iter()
                .map(|id| queue.graph().nodes[node_index[id]].clone())
                .collect();
            let mut handles = Vec::new();

            for node in node_clones {
                let span = info_span!("exec_node", node_id = %node.id, kind = ?node.kind);
                let cancel_token = cancel.cloned();
                handles.push(
                    async move {
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
                                result = executor.execute_node(&node) => {
                                    (node.id, result)
                                }
                            }
                        } else {
                            let result = executor.execute_node(&node).await;
                            (node.id, result)
                        }
                    }
                    .instrument(span),
                );
            }

            let results: Vec<_> = stream::iter(handles)
                .buffer_unordered(self.max_concurrent)
                .collect()
                .await;

            for (node_id, exec_result) in results {
                let latency = exec_result.latency_ms;
                if let Some(ref usage) = exec_result.usage {
                    total_tokens += usage.total_tokens as u64;
                    total_cost += usage.prompt_tokens as f64 * COST_PER_INPUT_TOKEN
                        + usage.completion_tokens as f64 * COST_PER_OUTPUT_TOKEN;

                    // Check per-request budget envelope after accumulating usage
                    if let Some(ref envelope) = instance.budget_envelope {
                        let cost_millicosts = (usage.prompt_tokens as f64 * COST_PER_INPUT_TOKEN
                            + usage.completion_tokens as f64 * COST_PER_OUTPUT_TOKEN) * 1000.0;
                        if let Err(ref e) = envelope.record_and_check(cost_millicosts as u64, usage.total_tokens as u64) {
                            info!(node_id = ?node_id, error = %e, "Budget envelope breached; stopping further execution");
                            for node in &queue.graph().nodes {
                                if matches!(instance.node_states.get(&node.id), Some(NodeState::Pending) | None) {
                                    instance.node_states.insert(node.id, NodeState::Skipped);
                                }
                            }
                            let total_latency = start.elapsed().as_millis() as u64;
                            return Ok(ExecutionResult {
                                instance_id: instance.instance_id,
                                success: false,
                                outputs: instance.outputs.clone(),
                                total_latency_ms: total_latency,
                                total_cost,
                                total_tokens,
                                terminal_node_id: instance.terminal_node_id,
                                final_output: instance.final_output.clone(),
                                stored_artifacts: Vec::new(),
                            });
                        }
                    }
                }
                match exec_result.state {
                    NodeState::Succeeded => {
                        retry_counts.remove(&node_id);
                        retry_backoffs.remove(&node_id);
                        info!(node_id = ?node_id, latency_ms = latency, "Node succeeded");
                        instance.node_states.insert(node_id, NodeState::Succeeded);
                        let output_val = exec_result.output.clone().unwrap_or(serde_json::Value::Null);
                        instance.outputs.insert(node_id, output_val.clone());

                        // Track terminal node output
                        instance.terminal_node_id = Some(node_id);
                        if output_val != serde_json::Value::Null {
                            instance.final_output = Some(output_val);
                        }

                        let node_kind = Some(queue.graph().nodes[node_index[&node_id]].kind.clone());

                        let edges: Vec<_> = queue.graph().edges.iter()
                            .filter(|e| e.from == node_id || e.to == node_id)
                            .cloned()
                            .collect();

                        match node_kind {
                            Some(ExecutionNodeKind::Conditional) => {
                                queue.mark_conditional_completed(node_id);
                                let result_val = instance.outputs.get(&node_id)
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("true");
                                for edge in &edges {
                                    if edge.from == node_id {
                                        let matches = match edge.condition.as_deref() {
                                            Some(cond) => cond == result_val,
                                            None => true,
                                        };
                                        if matches {
                                            queue.activate_edge(edge.from, edge.to);
                                        }
                                    }
                                }
                            }
                            Some(ExecutionNodeKind::Loop) => {
                                queue.mark_completed(node_id);
                                let should_continue = instance.outputs.get(&node_id)
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let outgoing: Vec<_> = edges.iter()
                                    .filter(|e| e.from == node_id)
                                    .cloned()
                                    .collect();
                                if should_continue {
                                    let body_ids: Vec<Uuid> = outgoing.iter()
                                        .filter(|e| e.condition.as_deref() != Some("exit"))
                                        .map(|e| e.to)
                                        .collect();
                                    for &body_id in &body_ids {
                                        instance.node_states.insert(body_id, NodeState::Pending);
                                    }
                                    queue.reset_loop_body(&body_ids);
                                } else {
                                    for edge in &outgoing {
                                        if edge.condition.as_deref() == Some("exit") {
                                            queue.activate_edge(edge.from, edge.to);
                                        }
                                    }
                                }
                            }
                            _ => {
                                queue.mark_completed(node_id);
                                let has_loop_back = edges.iter()
                                    .any(|e| e.from == node_id && e.condition.as_deref() == Some("loop"));
                                let loop_target = edges.iter()
                                    .find(|e| e.from == node_id && e.condition.as_deref() == Some("loop"))
                                    .map(|e| e.to);
                                if has_loop_back {
                                    if let Some(loop_node_id) = loop_target {
                                        let iter_count = loop_iterations.entry(loop_node_id).or_insert(0);
                                        let max_iters = queue.graph().nodes[node_index[&loop_node_id]]
                                            .config
                                            .get("max_iterations")
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(10) as u32;
                                        if *iter_count < max_iters {
                                            *iter_count += 1;
                                            info!(
                                                loop_node_id = ?loop_node_id,
                                                iteration = *iter_count,
                                                max = max_iters,
                                                "Loop iteration"
                                            );
                                            let loop_outgoing: Vec<_> = queue.graph().edges.iter()
                                                .filter(|e| e.from == loop_node_id)
                                                .cloned()
                                                .collect();
                                            let body_ids: Vec<Uuid> = loop_outgoing.iter()
                                                .filter(|e| e.condition.as_deref() != Some("exit"))
                                                .map(|e| e.to)
                                                .collect();
                                            for &body_id in &body_ids {
                                                instance.node_states.insert(body_id, NodeState::Pending);
                                            }
                                            queue.reset_loop_body(&body_ids);
                                            instance.node_states.insert(loop_node_id, NodeState::Pending);
                                            queue.reset_ready(loop_node_id);
                                            queue.activate_edge(node_id, loop_node_id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    NodeState::Failed(reason) => {
                        info!(node_id = ?node_id, reason = %reason, latency_ms = latency, "Node failed");
                        let retries = retry_counts.entry(node_id).or_insert(0);

                        let node_config = Some(queue.graph().nodes[node_index[&node_id]].clone());

                        if let Some(node) = node_config {
                            if *retries < node.retry_policy.max_retries {
                                *retries += 1;
                                info!(
                                    node_id = ?node_id,
                                    attempt = *retries,
                                    max = node.retry_policy.max_retries,
                                    "Retrying node after backoff"
                                );
                                if node.retry_policy.backoff_ms > 0 {
                                    let max_ms = node.retry_policy
                                        .backoff_ms
                                        .saturating_mul(10);
                                    let backoff = retry_backoffs
                                        .entry(node_id)
                                        .or_insert_with(|| Backoff::new(
                                            node.retry_policy.backoff_ms,
                                            max_ms,
                                        ));
                                    tokio::time::sleep(backoff.next()).await;
                                }
                                instance.node_states.insert(node_id, NodeState::Pending);
                                queue.reset_ready(node_id);
                            } else {
                                retry_backoffs.remove(&node_id);
                                if let Some(ref fallback) = node.fallback {
                                    info!(
                                        node_id = ?node_id,
                                        fallback_model = %fallback.model,
                                        "Attempting fallback execution"
                                    );
                                    let mut fallback_node = node.clone();
                                    fallback_node.model = fallback.model.clone();
                                    let fb_result = executor.execute_node(&fallback_node).await;
                                    match fb_result.state {
                                        NodeState::Succeeded => {
                                            info!(node_id = ?node_id, "Fallback succeeded");
                                            instance
                                                .node_states
                                                .insert(node_id, NodeState::Succeeded);
                                            queue.mark_completed(node_id);
                                            let fb_out = fb_result.output.clone().unwrap_or(serde_json::Value::Null);
                                            instance.outputs.insert(node_id, fb_out.clone());
                                            instance.terminal_node_id = Some(node_id);
                                            if fb_out != serde_json::Value::Null {
                                                instance.final_output = Some(fb_out);
                                            }
                                        }
                                        NodeState::Failed(fb_reason) => {
                                            instance.node_states.insert(
                                                node_id,
                                                NodeState::Failed(format!(
                                                    "Fallback failed: {}",
                                                    fb_reason
                                                )),
                                            );
                                            queue.mark_failed(node_id);
                                        }
                                        _ => {
                                            instance.node_states.insert(
                                                node_id,
                                                NodeState::Succeeded,
                                            );
                                            queue.mark_completed(node_id);
                                        }
                                    }
                                } else {
                                    instance
                                        .node_states
                                        .insert(node_id, NodeState::Failed(reason));
                                    queue.mark_failed(node_id);
                                }
                            }
                        } else {
                            instance
                                .node_states
                                .insert(node_id, NodeState::Failed(reason));
                            queue.mark_failed(node_id);
                        }
                    }
                    _ => {}
                }
            }
        }

        let total_latency = start.elapsed().as_millis() as u64;
        let success = instance
            .node_states
            .values()
            .all(|s| matches!(s, NodeState::Succeeded | NodeState::Skipped));

        Ok(ExecutionResult {
            instance_id: instance.instance_id,
            success,
            outputs: instance.outputs.clone(),
            total_latency_ms: total_latency,
            total_cost,
            total_tokens,
            terminal_node_id: instance.terminal_node_id,
            final_output: instance.final_output.clone(),
            stored_artifacts: Vec::new(),
        })
    }
}
