use async_trait::async_trait;
use crate::types::{ExecutionGraph, ExecutionInstance, ExecutionResult, ReservationId};

pub mod default;
pub mod work_queue;

#[async_trait]
pub trait Scheduler: Send + Sync {
    fn schedule(&self, graph: ExecutionGraph, reservation: ReservationId) -> ExecutionInstance;
    async fn run(
        &self,
        instance: &mut ExecutionInstance,
        executor: &dyn crate::executor::Executor,
    ) -> Result<ExecutionResult, crate::types::SchedulerError>;
}
