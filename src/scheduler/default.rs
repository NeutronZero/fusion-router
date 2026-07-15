use std::collections::HashMap;
use std::time::Instant;
use async_trait::async_trait;
use futures::future::join_all;
use tracing::{info, info_span, Instrument};
use uuid::Uuid;

use super::work_queue::WorkQueue;
use super::Scheduler;
use crate::executor::Executor;
use crate::types::{
    ExecutionGraph, ExecutionInstance, ExecutionResult, NodeState, ReservationId, SchedulerError,
};

pub struct DefaultScheduler;

impl DefaultScheduler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Scheduler for DefaultScheduler {
    fn schedule(&self, graph: ExecutionGraph, reservation: ReservationId) -> ExecutionInstance {
        let node_states: HashMap<Uuid, NodeState> = graph
            .nodes
            .iter()
            .map(|n| (n.id, NodeState::Pending))
            .collect();

        ExecutionInstance {
            instance_id: Uuid::new_v4(),
            graph,
            node_states,
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
        let total_tokens: u64 = 0;
        let total_cost: f64 = 0.0;
        let mut retry_counts: HashMap<Uuid, u32> = HashMap::new();

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
                let span = info_span!("exec_node", node_id = %node.id, strategy = ?node.strategy);
                handles.push(
                    async move {
                        let node_start = Instant::now();
                        let result = executor.execute_node(&node).await;
                        let latency = node_start.elapsed().as_millis() as u64;
                        (node.id, result, latency)
                    }
                    .instrument(span),
                );
            }

            let results = join_all(handles).await;

            for (node_id, result, latency) in results {
                match result {
                    Ok(NodeState::Succeeded) => {
                        info!(node_id = ?node_id, latency_ms = latency, "Node succeeded");
                        instance.node_states.insert(node_id, NodeState::Succeeded);
                        queue.mark_completed(node_id);
                    }
                    Ok(NodeState::Failed(reason)) => {
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
                                match executor.execute_node(&fallback_node).await {
                                    Ok(NodeState::Succeeded) => {
                                        info!(node_id = ?node_id, "Fallback succeeded");
                                        instance
                                            .node_states
                                            .insert(node_id, NodeState::Succeeded);
                                        queue.mark_completed(node_id);
                                    }
                                    Ok(NodeState::Failed(fb_reason)) => {
                                        instance.node_states.insert(
                                            node_id,
                                            NodeState::Failed(format!(
                                                "Fallback failed: {}",
                                                fb_reason
                                            )),
                                        );
                                        queue.mark_failed(node_id);
                                    }
                                    Ok(_) => {
                                        instance.node_states.insert(
                                            node_id,
                                            NodeState::Succeeded,
                                        );
                                        queue.mark_completed(node_id);
                                    }
                                    Err(e) => {
                                        instance.node_states.insert(
                                            node_id,
                                            NodeState::Failed(format!(
                                                "Fallback error: {}",
                                                e
                                            )),
                                        );
                                        queue.mark_failed(node_id);
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
                    Err(e) => {
                        info!(node_id = ?node_id, error = %e, "Node execution error");
                        instance
                            .node_states
                            .insert(node_id, NodeState::Failed(e.to_string()));
                        queue.mark_failed(node_id);
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
