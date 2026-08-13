//! Phase 6.2.2 — resource bridge: `src/resource` → `fusion_kernel::resource`.
//!
//! `fusion_compiler`'s passes take `Arc<dyn fusion_kernel::resource::ResourceManager>`
//! (scalar-based budget checks). The monolith's `ResourceManager` is graph-shaped
//! (`can_afford(&ExecutionGraph)`). `KernelResourceManager` adapts the scalar
//! kernel view onto the production monolith manager without changing either trait.
//! The `Quota` copy is a two-field projection (kernel quota is scope-minimal);
//! live spend/accounting always reads through to the monolith implementation.

use std::sync::Arc;
use async_trait::async_trait;
use uuid::Uuid;
use crate::resource::ResourceManager;
use crate::types::{ExecutionGraph, GraphMetadata};

pub struct KernelResourceManager {
    inner: Arc<dyn ResourceManager>,
    quota: fusion_kernel::resource::Quota,
}

impl KernelResourceManager {
    pub fn new(inner: Arc<dyn ResourceManager>) -> Self {
        let quota = inner.quota();
        let (max_daily_cost, max_daily_tokens) = (quota.max_daily_cost, quota.max_daily_tokens);
        Self {
            inner,
            quota: fusion_kernel::resource::Quota {
                max_daily_cost,
                max_daily_tokens,
            },
        }
    }

    fn graph_for(estimated_cost: f64, estimated_tokens: u64) -> ExecutionGraph {
        ExecutionGraph {
            graph_id: Uuid::nil(),
            nodes: vec![],
            edges: vec![],
            metadata: GraphMetadata {
                estimated_cost,
                estimated_tokens,
                max_depth: 1,
                node_count: 0,
            },
            primitive_graph_hash: 0,
            total_tokens: 0,
            total_cost: 0,
        }
    }
}

#[async_trait]
impl fusion_kernel::resource::ResourceManager for KernelResourceManager {
    async fn can_afford(&self, estimated_cost: f64, estimated_tokens: u64) -> bool {
        self.inner
            .can_afford(&Self::graph_for(estimated_cost, estimated_tokens))
            .await
    }

    async fn try_reserve(&self, estimated_cost: f64, estimated_tokens: u64) -> bool {
        self.inner
            .try_reserve(&Self::graph_for(estimated_cost, estimated_tokens))
            .await
    }

    async fn release(&self, estimated_cost: f64, estimated_tokens: u64) -> anyhow::Result<()> {
        self.inner
            .release(&Self::graph_for(estimated_cost, estimated_tokens))
            .await
    }

    fn quota(&self) -> &fusion_kernel::resource::Quota {
        &self.quota
    }

    fn spent_cost(&self) -> f64 {
        self.inner.spent_cost()
    }

    fn spent_tokens(&self) -> u64 {
        self.inner.spent_tokens()
    }

    async fn record_usage(&self, cost_millicosts: u64, tokens: u64) {
        self.inner.record_usage(cost_millicosts, tokens).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::DefaultResourceManager;
    use crate::types::Quota;
    use fusion_kernel::resource::ResourceManager as _;

    fn manager() -> KernelResourceManager {
        KernelResourceManager::new(Arc::new(DefaultResourceManager::new(Quota {
            max_daily_cost: 1.0,
            max_daily_tokens: 1000,
            max_concurrent: 10,
            provider_limits: std::collections::HashMap::new(),
        })))
    }

    #[tokio::test]
    async fn scalar_checks_delegate_to_graph_shaped_manager() {
        let rm = manager();
        assert!(rm.can_afford(0.5, 500).await);
        assert!(!rm.can_afford(1.1, 500).await, "over cost quota");
        assert!(!rm.can_afford(0.5, 1100).await, "over token quota");
    }

    #[tokio::test]
    async fn reserve_and_release_delegate() {
        let rm = manager();
        assert!(rm.try_reserve(0.6, 600).await);
        assert!(!rm.try_reserve(0.6, 600).await, "quota exhausted after reserve");
        rm.release(0.6, 600).await.unwrap();
        assert!(rm.try_reserve(0.6, 600).await, "release frees quota");
    }

    #[test]
    fn quota_projects_two_field_view() {
        let rm = manager();
        assert_eq!(rm.quota().max_daily_cost, 1.0);
        assert_eq!(rm.quota().max_daily_tokens, 1000);
    }

    #[tokio::test]
    async fn spend_reads_through_to_inner() {
        let rm = manager();
        rm.try_reserve(0.25, 250).await;
        assert_eq!(rm.spent_cost(), 0.25);
        assert_eq!(rm.spent_tokens(), 250);
    }
}
