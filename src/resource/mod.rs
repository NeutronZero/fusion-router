use async_trait::async_trait;
use crate::types::{ExecutionGraph, Quota};

#[async_trait]
pub trait ResourceManager: Send + Sync {
    async fn can_afford(&self, graph: &ExecutionGraph) -> bool;
    async fn reserve(&self, graph: &ExecutionGraph) -> anyhow::Result<()>;
    async fn release(&self, graph: &ExecutionGraph) -> anyhow::Result<()>;
    fn quota(&self) -> &Quota;
}

pub struct DefaultResourceManager {
    quota: Quota,
}

impl DefaultResourceManager {
    pub fn new(quota: Quota) -> Self {
        Self { quota }
    }
}

#[async_trait]
impl ResourceManager for DefaultResourceManager {
    async fn can_afford(&self, graph: &ExecutionGraph) -> bool {
        graph.metadata.estimated_cost <= self.quota.max_daily_cost
            && graph.metadata.estimated_tokens <= self.quota.max_daily_tokens as u64
    }

    async fn reserve(&self, _graph: &ExecutionGraph) -> anyhow::Result<()> {
        Ok(())
    }

    async fn release(&self, _graph: &ExecutionGraph) -> anyhow::Result<()> {
        Ok(())
    }

    fn quota(&self) -> &Quota {
        &self.quota
    }
}
