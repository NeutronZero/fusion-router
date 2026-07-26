use std::sync::Arc;
use uuid::Uuid;
use crate::types::ExecutionGraph;
use super::ResourceManager;

pub struct ResourceGuard {
    pub request_id: Uuid,
    pub graph: ExecutionGraph,
    pub resource_manager: Arc<dyn ResourceManager>,
    pub committed: bool,
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
        }
    }

    pub fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for ResourceGuard {
    fn drop(&mut self) {
        if !self.committed {
            let resource_manager = self.resource_manager.clone();
            let graph = self.graph.clone();
            let request_id = self.request_id;
            tracing::warn!(
                request_id = %request_id,
                "ResourceGuard dropped without commit; releasing reserved quota"
            );
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = resource_manager.release(&graph).await;
                });
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
            max_daily_cost: 10.0,
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
                estimated_cost: 2.0,
                estimated_tokens: 200,
                max_depth: 1,
                node_count: 0,
            },
            total_tokens: 200,
            total_cost: 2,
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
            max_daily_cost: 10.0,
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
                estimated_cost: 2.0,
                estimated_tokens: 200,
                max_depth: 1,
                node_count: 0,
            },
            total_tokens: 200,
            total_cost: 2,
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
}
