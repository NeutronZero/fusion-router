use async_trait::async_trait;
use fusion_core::PlatformError;

#[async_trait]
pub trait Scheduler: Send + Sync {
    async fn schedule(&self, graph_id: &str) -> Result<Vec<String>, PlatformError>;
}

pub struct SequentialScheduler;

#[async_trait]
impl Scheduler for SequentialScheduler {
    async fn schedule(&self, graph_id: &str) -> Result<Vec<String>, PlatformError> {
        Ok(vec![format!("seq_node_1_{graph_id}"), format!("seq_node_2_{graph_id}")])
    }
}

pub struct ParallelScheduler;

#[async_trait]
impl Scheduler for ParallelScheduler {
    async fn schedule(&self, graph_id: &str) -> Result<Vec<String>, PlatformError> {
        Ok(vec![format!("par_branch_a_{graph_id}"), format!("par_branch_b_{graph_id}")])
    }
}

pub struct CostOptimizedScheduler;

#[async_trait]
impl Scheduler for CostOptimizedScheduler {
    async fn schedule(&self, graph_id: &str) -> Result<Vec<String>, PlatformError> {
        Ok(vec![format!("cheap_node_1_{graph_id}"), format!("cheap_node_2_{graph_id}")])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sequential_scheduler() {
        let scheduler = SequentialScheduler;
        let nodes = scheduler.schedule("g1").await.expect("Schedule");
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0], "seq_node_1_g1");
    }

    #[tokio::test]
    async fn test_parallel_scheduler() {
        let scheduler = ParallelScheduler;
        let nodes = scheduler.schedule("g1").await.expect("Schedule");
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0], "par_branch_a_g1");
    }
}
