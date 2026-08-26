use super::ResourceManager;
use crate::types::ExecutionGraph;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use uuid::Uuid;

pub struct ResourceGuard {
    pub request_id: Uuid,
    pub graph: ExecutionGraph,
    pub resource_manager: Arc<dyn ResourceManager>,
    pub committed: bool,
    runtime: Option<tokio::runtime::Handle>,
}

impl ResourceGuard {
    pub fn new(
        request_id: Uuid,
        graph: ExecutionGraph,
        resource_manager: Arc<dyn ResourceManager>,
    ) -> Self {
        Self {
            request_id,
            graph,
            resource_manager,
            committed: false,
            // Capture the runtime at construction (all guards are created
            // inside a Tokio context). Without this, a guard dropped from a
            // non-runtime thread or after runtime teardown can never return
            // the reserved quota.
            runtime: tokio::runtime::Handle::try_current().ok(),
        }
    }

    pub fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for ResourceGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        tracing::warn!(
            request_id = %self.request_id,
            reserved_tokens = self.graph.metadata.estimated_tokens,
            reserved_cost_nanos = self.graph.metadata.estimated_cost.as_nanos(),
            "ResourceGuard dropped without commit; releasing reserved quota"
        );

        let request_id = self.request_id;
        let mut spawned = false;
        if let Some(handle) = &self.runtime {
            let resource_manager = self.resource_manager.clone();
            let graph = self.graph.clone();
            spawned = catch_unwind(AssertUnwindSafe(|| {
                handle.spawn(async move {
                    if let Err(e) = resource_manager.release(&graph).await {
                        tracing::error!(
                            request_id = %request_id,
                            error = %e,
                            reserved_tokens = graph.metadata.estimated_tokens,
                            "Failed to release reserved quota on guard drop"
                        );
                    }
                });
                true
            }))
            .unwrap_or(false);
        }

        if !spawned {
            // No runtime captured (or the captured runtime rejected the
            // spawn): best-effort synchronous release. Guarded so neither a
            // panic in the manager nor a failed release can unwind out of
            // `drop`.
            let resource_manager = self.resource_manager.clone();
            let graph = self.graph.clone();
            let result = catch_unwind(AssertUnwindSafe(|| {
                futures::executor::block_on(resource_manager.release(&graph))
            }));
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::error!(
                    request_id = %request_id,
                    error = %e,
                    reserved_tokens = self.graph.metadata.estimated_tokens,
                    "Synchronous release of reserved quota failed on guard drop"
                ),
                Err(_) => tracing::error!(
                    request_id = %request_id,
                    reserved_tokens = self.graph.metadata.estimated_tokens,
                    "Synchronous release of reserved quota panicked on guard drop"
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::DefaultResourceManager;
    use crate::types::*;

    #[tokio::test]
    async fn test_resource_guard_drop_releases_quota() {
        let quota = Quota {
            max_daily_cost: NanoUSD::from_nanos(10_000_000_000),
            max_daily_tokens: 1000,
            max_concurrent: 10,
            provider_limits: std::collections::HashMap::new(),
        };
        let manager: Arc<dyn ResourceManager> = Arc::new(DefaultResourceManager::new(quota));

        let graph = ExecutionGraph {
            graph_id: Uuid::new_v4(),
            nodes: vec![],
            edges: vec![],
            metadata: GraphMetadata {
                policy_version: 0,
                estimated_cost: NanoUSD::from_nanos(2_000_000_000),
                estimated_tokens: 200,
                max_depth: 1,
                node_count: 0,
            },
            total_tokens: 200,
            total_cost: NanoUSD::from_nanos(2_000_000_000),
            primitive_graph_hash: 0,
        };

        let reserved = manager.try_reserve(&graph).await;
        assert!(reserved);
        assert_eq!(manager.spent_tokens(), 200);

        {
            let _guard = ResourceGuard::new(Uuid::new_v4(), graph.clone(), manager.clone());
            // Drops without commit here
        }

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(manager.spent_tokens(), 0);
    }

    #[tokio::test]
    async fn test_resource_guard_commit_retains_quota() {
        let quota = Quota {
            max_daily_cost: NanoUSD::from_nanos(10_000_000_000),
            max_daily_tokens: 1000,
            max_concurrent: 10,
            provider_limits: std::collections::HashMap::new(),
        };
        let manager: Arc<dyn ResourceManager> = Arc::new(DefaultResourceManager::new(quota));

        let graph = ExecutionGraph {
            graph_id: Uuid::new_v4(),
            nodes: vec![],
            edges: vec![],
            metadata: GraphMetadata {
                policy_version: 0,
                estimated_cost: NanoUSD::from_nanos(2_000_000_000),
                estimated_tokens: 200,
                max_depth: 1,
                node_count: 0,
            },
            total_tokens: 200,
            total_cost: NanoUSD::from_nanos(2_000_000_000),
            primitive_graph_hash: 0,
        };

        let reserved = manager.try_reserve(&graph).await;
        assert!(reserved);

        {
            let mut guard = ResourceGuard::new(Uuid::new_v4(), graph.clone(), manager.clone());
            guard.commit();
        }

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(manager.spent_tokens(), 200);
    }

    fn quota() -> Quota {
        Quota {
            max_daily_cost: NanoUSD::from_nanos(10_000_000_000),
            max_daily_tokens: 1000,
            max_concurrent: 10,
            provider_limits: std::collections::HashMap::new(),
        }
    }

    fn graph() -> ExecutionGraph {
        ExecutionGraph {
            graph_id: Uuid::new_v4(),
            nodes: vec![],
            edges: vec![],
            metadata: GraphMetadata {
                policy_version: 0,
                estimated_cost: NanoUSD::from_nanos(2_000_000_000),
                estimated_tokens: 200,
                max_depth: 1,
                node_count: 0,
            },
            total_tokens: 200,
            total_cost: NanoUSD::from_nanos(2_000_000_000),
            primitive_graph_hash: 0,
        }
    }

    #[test]
    fn test_resource_guard_drop_without_runtime_releases_synchronously() {
        use futures::executor::block_on;
        let manager: Arc<dyn ResourceManager> = Arc::new(DefaultResourceManager::new(quota()));
        let graph = graph();

        block_on(manager.try_reserve(&graph));
        assert_eq!(manager.spent_tokens(), 200);

        // Constructed and dropped with no Tokio runtime on this thread.
        {
            let _guard = ResourceGuard::new(Uuid::new_v4(), graph.clone(), manager.clone());
        }

        assert_eq!(
            manager.spent_tokens(),
            0,
            "runtime-less guard must release synchronously at drop"
        );
        assert_eq!(manager.spent_cost().as_nanos(), 0);
    }

    #[test]
    fn test_resource_guard_commit_off_runtime_retains_quota() {
        use futures::executor::block_on;
        let manager: Arc<dyn ResourceManager> = Arc::new(DefaultResourceManager::new(quota()));
        let graph = graph();

        block_on(manager.try_reserve(&graph));

        {
            let mut guard = ResourceGuard::new(Uuid::new_v4(), graph, manager.clone());
            guard.commit();
        }

        assert_eq!(
            manager.spent_tokens(),
            200,
            "committed guard must retain the reservation even without a runtime"
        );
    }
}
