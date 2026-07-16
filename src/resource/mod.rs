use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use crate::types::{ExecutionGraph, Quota};

#[async_trait]
pub trait ResourceManager: Send + Sync {
    async fn can_afford(&self, graph: &ExecutionGraph) -> bool;
    async fn reserve(&self, graph: &ExecutionGraph) -> anyhow::Result<()>;
    async fn release(&self, graph: &ExecutionGraph) -> anyhow::Result<()>;
    fn quota(&self) -> &Quota;
    fn spent_cost(&self) -> f64;
    fn spent_tokens(&self) -> u64;
}

pub struct DefaultResourceManager {
    quota: Quota,
    used_cost: AtomicU64,
    used_tokens: AtomicU64,
}

impl DefaultResourceManager {
    pub fn new(quota: Quota) -> Self {
        Self {
            quota,
            used_cost: AtomicU64::new(0),
            used_tokens: AtomicU64::new(0),
        }
    }
}

#[async_trait]
impl ResourceManager for DefaultResourceManager {
    async fn can_afford(&self, graph: &ExecutionGraph) -> bool {
        let cost = (graph.metadata.estimated_cost * 1000.0) as u64;
        let tokens = graph.metadata.estimated_tokens;
        let current_cost = self.used_cost.load(Ordering::Relaxed);
        let current_tokens = self.used_tokens.load(Ordering::Relaxed);
        let max_cost = (self.quota.max_daily_cost * 1000.0) as u64;
        let max_tokens = self.quota.max_daily_tokens;
        (current_cost + cost <= max_cost) && (current_tokens + tokens <= max_tokens)
    }

    async fn reserve(&self, graph: &ExecutionGraph) -> anyhow::Result<()> {
        let cost = (graph.metadata.estimated_cost * 1000.0) as u64;
        let tokens = graph.metadata.estimated_tokens;
        self.used_cost.fetch_add(cost, Ordering::Relaxed);
        self.used_tokens.fetch_add(tokens, Ordering::Relaxed);
        Ok(())
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
        self.used_cost.load(Ordering::Relaxed) as f64 / 1000.0
    }

    fn spent_tokens(&self) -> u64 {
        self.used_tokens.load(Ordering::Relaxed)
    }
}
