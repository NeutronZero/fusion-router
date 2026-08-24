use crate::types::{ExecutionGraph, ExecutionInstance, ExecutionResult, ReservationId};
use async_trait::async_trait;

pub mod connector_health;
pub mod connector_resolver;
pub mod connector_subscriber;
pub mod crates_adapter;
pub mod default;

#[async_trait]
pub trait Scheduler: Send + Sync {
    fn schedule(&self, graph: ExecutionGraph, reservation: ReservationId) -> ExecutionInstance;
    async fn run(
        &self,
        instance: &mut ExecutionInstance,
        executor: &dyn crate::executor::Executor,
    ) -> Result<ExecutionResult, crate::types::SchedulerError>;

    async fn run_with_cancellation(
        &self,
        instance: &mut ExecutionInstance,
        executor: &dyn crate::executor::Executor,
        cancellation_token: &tokio_util::sync::CancellationToken,
    ) -> Result<ExecutionResult, crate::types::SchedulerError> {
        let _ = cancellation_token;
        self.run(instance, executor).await
    }
}
