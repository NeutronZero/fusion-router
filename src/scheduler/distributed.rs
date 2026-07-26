use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::executor::Executor;
use crate::scheduler::Scheduler;
use crate::scheduler::default::DefaultScheduler;
use crate::types::{
    ExecutionGraph, ExecutionInstance, ExecutionResult, ReservationId, SchedulerError,
};

#[derive(Debug, Clone)]
pub struct WorkerNode {
    pub id: String,
    pub address: String,
    pub active_tasks: usize,
    pub capacity: usize,
}

impl WorkerNode {
    pub fn new(id: String, address: String, capacity: usize) -> Self {
        Self {
            id,
            address,
            active_tasks: 0,
            capacity,
        }
    }
}

#[derive(Default, Clone)]
pub struct RemoteWorkerPool {
    workers: Arc<RwLock<HashMap<String, WorkerNode>>>,
}

impl RemoteWorkerPool {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_worker(&self, worker: WorkerNode) {
        let mut w = self.workers.write().await;
        w.insert(worker.id.clone(), worker);
    }

    pub async fn remove_worker(&self, worker_id: &str) {
        let mut w = self.workers.write().await;
        w.remove(worker_id);
    }

    pub async fn get_workers(&self) -> Vec<WorkerNode> {
        let w = self.workers.read().await;
        w.values().cloned().collect()
    }
}

pub struct DistributedScheduler {
    #[allow(dead_code)]
    pool: RemoteWorkerPool,
    local_fallback: DefaultScheduler,
}

impl DistributedScheduler {
    pub fn new(pool: RemoteWorkerPool) -> Self {
        Self {
            pool,
            local_fallback: DefaultScheduler::default(),
        }
    }
}

#[async_trait]
impl Scheduler for DistributedScheduler {
    fn schedule(&self, graph: ExecutionGraph, reservation: ReservationId) -> ExecutionInstance {
        self.local_fallback.schedule(graph, reservation)
    }

    async fn run(
        &self,
        instance: &mut ExecutionInstance,
        executor: &dyn Executor,
    ) -> Result<ExecutionResult, SchedulerError> {
        self.local_fallback.run(instance, executor).await
    }

    async fn run_with_cancellation(
        &self,
        instance: &mut ExecutionInstance,
        executor: &dyn Executor,
        cancellation_token: &CancellationToken,
    ) -> Result<ExecutionResult, SchedulerError> {
        self.local_fallback.run_with_cancellation(instance, executor, cancellation_token).await
    }
}
