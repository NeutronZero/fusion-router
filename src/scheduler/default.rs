//! Production scheduler entry point (Phase 6.3b flip).
//!
//! `DefaultScheduler` now delegates run execution to
//! `fusion_scheduler::DefaultScheduler`. The src-side executor is adapted
//! through `CratesExecutorAdapter`, which carries the two behaviors the
//! monolith loop used to own (retry/fallback, terminal-output tracking).
//! The budget envelope is enforced inside the crates loop
//! (`run_with_budget` / `run_with_cancellation_and_budget`).
//!
//! `schedule` remains src-side: it just allocates an `ExecutionInstance`
//! describing the graph; only running is delegated.

use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::executor::Executor;
use crate::scheduler::crates_adapter::CratesExecutorAdapter;
use crate::types::{
    ExecutionGraph, ExecutionInstance, ExecutionResult, NodeState, ReservationId, SchedulerError,
};

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

#[async_trait::async_trait]
impl crate::scheduler::Scheduler for DefaultScheduler {
    #[tracing::instrument(skip(self, graph), fields(node_count = graph.nodes.len()))]
    fn schedule(&self, graph: ExecutionGraph, reservation: ReservationId) -> ExecutionInstance {
        let mut node_states = HashMap::with_capacity(graph.nodes.len());
        for node in &graph.nodes {
            node_states.insert(node.id, NodeState::Pending);
        }

        ExecutionInstance {
            instance_id: Uuid::new_v4(),
            graph: std::sync::Arc::new(graph),
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
        self.run_crates(instance, executor, None).await
    }

    #[tracing::instrument(skip(self, instance, executor, cancellation_token), fields(graph_id = %instance.graph.graph_id))]
    async fn run_with_cancellation(
        &self,
        instance: &mut ExecutionInstance,
        executor: &dyn Executor,
        cancellation_token: &CancellationToken,
    ) -> Result<ExecutionResult, SchedulerError> {
        self.run_crates(instance, executor, Some(cancellation_token)).await
    }
}

impl DefaultScheduler {
    // Delegated execution: fusion_scheduler drives the DAG; this function
    // maps the crates outcome back onto the src ExecutionInstance contract.
    async fn run_crates(
        &self,
        instance: &mut ExecutionInstance,
        executor: &dyn Executor,
        cancel: Option<&CancellationToken>,
    ) -> Result<ExecutionResult, SchedulerError> {
        let (adapter, tracker) = CratesExecutorAdapter::new(executor);
        let crates_scheduler = fusion_scheduler::DefaultScheduler::with_max_concurrent(self.max_concurrent);
        let graph = Arc::clone(&instance.graph);

        let outcome = match (cancel, instance.budget_envelope.as_ref()) {
            (Some(token), Some(envelope)) => {
                crates_scheduler
                    .run_with_cancellation_and_budget(graph, &adapter, token, envelope)
                    .await
            }
            (Some(token), None) => crates_scheduler.run_with_cancellation(graph, &adapter, token).await,
            (None, Some(envelope)) => crates_scheduler.run_with_budget(graph, &adapter, envelope).await,
            (None, None) => crates_scheduler.run(graph, &adapter).await,
        }
        .map_err(map_crates_error)?;

        // Overlay the crates outcome onto the instance: pre-seeded Pending
        // states for nodes that never ran are preserved (matches the
        // monolith's in-place mutation), success/failure states overwrite.
        let mut states = instance.node_states.clone();
        states.extend(outcome.node_states);
        instance.node_states = states;
        instance.outputs = outcome.outputs;

        let tracker_guard = tracker.lock().unwrap();
        instance.terminal_node_id = tracker_guard.terminal_node_id;
        instance.final_output = tracker_guard.final_output.clone();
        let terminal_node_id = instance.terminal_node_id;
        let final_output = instance.final_output.clone();
        drop(tracker_guard);

        Ok(ExecutionResult {
            instance_id: instance.instance_id,
            success: outcome.success,
            outputs: instance.outputs.clone(),
            total_latency_ms: outcome.total_latency_ms,
            total_cost: outcome.total_cost,
            total_tokens: outcome.total_tokens,
            terminal_node_id,
            final_output,
        })
    }
}

fn map_crates_error(err: fusion_core::PlatformError) -> SchedulerError {
    match err {
        fusion_core::PlatformError::Scheduler { code, message: _message, .. } if code == "CANCELLED" => {
            SchedulerError::Internal("Request cancelled by client".into())
        }
        fusion_core::PlatformError::Scheduler { message, .. } => SchedulerError::Internal(message),
        other => SchedulerError::Internal(other.to_string()),
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