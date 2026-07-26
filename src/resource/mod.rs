use async_trait::async_trait;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use crate::types::{ExecutionGraph, Quota};

pub mod cancelling_stream;
pub mod guard;
pub mod budget;
pub mod stream_meter;

pub use guard::ResourceGuard;
pub use budget::BudgetEnvelope;

#[async_trait]
pub trait ResourceManager: Send + Sync {
    async fn can_afford(&self, graph: &ExecutionGraph) -> bool;
    async fn try_reserve(&self, graph: &ExecutionGraph) -> bool;
    async fn release(&self, graph: &ExecutionGraph) -> anyhow::Result<()>;
    fn quota(&self) -> &Quota;
    fn spent_cost(&self) -> f64;
    fn spent_tokens(&self) -> u64;
}

pub struct DefaultResourceManager {
    quota: Quota,
    used_cost: AtomicU64,
    used_tokens: AtomicU64,
    reserve_lock: Mutex<()>,
}

impl DefaultResourceManager {
    pub fn new(quota: Quota) -> Self {
        Self {
            quota,
            used_cost: AtomicU64::new(0),
            used_tokens: AtomicU64::new(0),
            reserve_lock: Mutex::new(()),
        }
    }
}

#[async_trait]
impl ResourceManager for DefaultResourceManager {
    async fn can_afford(&self, graph: &ExecutionGraph) -> bool {
        let cost = (graph.metadata.estimated_cost * 1000.0) as u64;
        let tokens = graph.metadata.estimated_tokens;
        let current_cost = self.used_cost.load(Ordering::Acquire);
        let current_tokens = self.used_tokens.load(Ordering::Acquire);
        let max_cost = (self.quota.max_daily_cost * 1000.0) as u64;
        let max_tokens = self.quota.max_daily_tokens;
        (current_cost + cost <= max_cost) && (current_tokens + tokens <= max_tokens)
    }

    async fn try_reserve(&self, graph: &ExecutionGraph) -> bool {
        let cost = (graph.metadata.estimated_cost * 1000.0) as u64;
        let tokens = graph.metadata.estimated_tokens;
        let max_cost = (self.quota.max_daily_cost * 1000.0) as u64;
        let max_tokens = self.quota.max_daily_tokens;

        let _guard = self.reserve_lock.lock();
        let current_cost = self.used_cost.load(Ordering::Relaxed);
        let current_tokens = self.used_tokens.load(Ordering::Relaxed);

        if current_cost + cost > max_cost || current_tokens + tokens > max_tokens {
            return false;
        }

        self.used_cost.store(current_cost + cost, Ordering::Release);
        self.used_tokens.store(current_tokens + tokens, Ordering::Release);
        true
    }

    async fn release(&self, graph: &ExecutionGraph) -> anyhow::Result<()> {
        let cost = (graph.metadata.estimated_cost * 1000.0) as u64;
        let tokens = graph.metadata.estimated_tokens;
        self.used_cost.fetch_sub(cost, Ordering::Relaxed);
        self.used_tokens.fetch_sub(tokens, Ordering::Relaxed);
        Ok(())
    }

    fn quota(&self) -> &Quota {
        &self.quota
    }

    fn spent_cost(&self) -> f64 {
        self.used_cost.load(Ordering::Acquire) as f64 / 1000.0
    }

    fn spent_tokens(&self) -> u64 {
        self.used_tokens.load(Ordering::Acquire)
    }
}
