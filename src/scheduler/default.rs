use std::collections::HashMap;
use std::time::Instant;
use futures::future::join_all;
use tracing::{info, info_span, Instrument};
use uuid::Uuid;

use crate::executor::Executor;
use crate::scheduler::work_queue::WorkQueue;
use crate::types::{
    ExecutionGraph, ExecutionInstance, ExecutionNodeKind, ExecutionResult, NodeState,
    ReservationId, SchedulerError,
};

const COST_PER_INPUT_TOKEN: f64 = 0.002 / 1000.0;
const COST_PER_OUTPUT_TOKEN: f64 = 0.01 / 1000.0;

pub struct DefaultScheduler;

impl DefaultScheduler {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl crate::scheduler::Scheduler for DefaultScheduler {
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
        }
    }

    async fn run(
        &self,
        instance: &mut ExecutionInstance,
        executor: &dyn Executor,
    ) -> Result<ExecutionResult, SchedulerError> {
        let start = Instant::now();
        let mut total_tokens: u64 = 0;
        let mut total_cost: f64 = 0.0;
        let mut retry_counts: HashMap<Uuid, u32> = HashMap::new();
        let mut loop_iterations: HashMap<Uuid, u32> = HashMap::new();

        let mut queue = WorkQueue::new(instance.graph.clone());

        loop {
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
                .filter_map(|id| queue.graph().nodes.iter().find(|n| n.id == *id).cloned())
                .collect();
            let mut handles = Vec::new();

            for node in node_clones {
                let span = info_span!("exec_node", node_id = %node.id, kind = ?node.kind);
                handles.push(
                    async move {
                        let result = executor.execute_node(&node).await;
                        (node.id, result)
                    }
                    .instrument(span),
                );
            }

            let results = join_all(handles).await;

            for (node_id, exec_result) in results {
                let latency = exec_result.latency_ms;
                if let Some(ref usage) = exec_result.usage {
                    total_tokens += usage.total_tokens as u64;
                    total_cost += usage.prompt_tokens as f64 * COST_PER_INPUT_TOKEN
                        + usage.completion_tokens as f64 * COST_PER_OUTPUT_TOKEN;
                }
                match exec_result.state {
                    NodeState::Succeeded => {
                        info!(node_id = ?node_id, latency_ms = latency, "Node succeeded");
                        instance.node_states.insert(node_id, NodeState::Succeeded);

                        let node_kind = queue.graph().nodes.iter()
                            .find(|n| n.id == node_id)
                            .map(|n| n.kind.clone());

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
                                        let max_iters = queue.graph().nodes.iter()
                                            .find(|n| n.id == loop_node_id)
                                            .and_then(|n| n.config.get("max_iterations"))
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

                        let node_config =
                            queue.graph().nodes.iter().find(|n| n.id == node_id).cloned();

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
                                    tokio::time::sleep(std::time::Duration::from_millis(
                                        node.retry_policy.backoff_ms,
                                    ))
                                    .await;
                                }
                                instance.node_states.insert(node_id, NodeState::Pending);
                                queue.reset_ready(node_id);
                            } else if let Some(ref fallback) = node.fallback {
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
                        } else {
                            instance
                                .node_states
                                .insert(node_id, NodeState::Failed(reason));
                            queue.mark_failed(node_id);
                        }
                    }
                    _ => {
                        instance.node_states.insert(node_id, NodeState::Succeeded);
                        queue.mark_completed(node_id);
                    }
                }
            }
        }

        let success = !instance
            .node_states
            .values()
            .any(|s| matches!(s, NodeState::Failed(_)));

        let total_elapsed = start.elapsed().as_millis() as u64;

        Ok(ExecutionResult {
            instance_id: instance.instance_id,
            success,
            outputs: HashMap::new(),
            total_latency_ms: total_elapsed,
            total_cost,
            total_tokens,
        })
    }
}
