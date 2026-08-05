//! **SIMULATION** — Studio-sandbox scheduler (v0.14 UI vertical).
//!
//! All scheduler implementations return synthetic node-id strings; no DAG is
//! executed. NOT wired into the production `src/` monolith scheduler.
use async_trait::async_trait;
use fusion_core::PlatformError;
pub use fusion_placement::{ExecutionPlan, ExecutionPlanId, PlacementGraph};
use std::collections::HashMap;

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

// =========================================================================
// SPRINT 4: Distributed Scheduler (DAG Partitioning & ExecutionPlan Creation)
// =========================================================================

pub struct DistributedScheduler;

impl DistributedScheduler {
    pub fn new() -> Self {
        Self
    }

    pub fn create_plan(&self, placement_graph: &PlacementGraph) -> ExecutionPlan {
        let mut execution_order = Vec::new();
        let mut worker_assignments = HashMap::new();

        for node in &placement_graph.nodes {
            execution_order.push(node.id.clone());
            worker_assignments.insert(node.id.clone(), node.worker_id.clone());
        }

        ExecutionPlan {
            plan_id: ExecutionPlanId::new(),
            placement_id: placement_graph.placement_id.clone(),
            execution_id: placement_graph.execution_id.clone(),
            execution_order,
            worker_assignments,
            max_parallelism: 8,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

impl Default for DistributedScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_placement::PlacementNode;

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

    #[test]
    fn test_distributed_scheduler_creates_execution_plan_from_placement_graph() {
        let placement_graph = PlacementGraph {
            placement_id: fusion_placement::PlacementId::new(),
            execution_id: "exec_500".into(),
            nodes: vec![
                PlacementNode { id: "n1".into(), worker_id: "w1".into(), config: HashMap::new() },
                PlacementNode { id: "n2".into(), worker_id: "w2".into(), config: HashMap::new() },
            ],
            placement_policy: "locality-v1".into(),
        };

        let scheduler = DistributedScheduler::new();
        let plan = scheduler.create_plan(&placement_graph);

        assert_eq!(plan.execution_id, "exec_500");
        assert_eq!(plan.execution_order.len(), 2);
        assert_eq!(plan.worker_assignments.get("n1").unwrap(), "w1");
        assert_eq!(plan.worker_assignments.get("n2").unwrap(), "w2");
    }
}
