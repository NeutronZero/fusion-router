//! Scheduler Intelligence Operational Validation Suite (v0.14.4)
//!
//! Validates DAG execution plan quality, worker assignment completeness, and scheduling latency.

use fusion_placement::{PlacementGraph, PlacementId, PlacementNode};
use fusion_scheduler::DistributedScheduler;
use std::collections::HashMap;

#[test]
fn test_scheduler_creates_canonical_execution_plan() {
    let placement_graph = PlacementGraph {
        placement_id: PlacementId::new(),
        execution_id: "exec_777".into(),
        nodes: vec![
            PlacementNode { id: "n1".into(), worker_id: "w1".into(), config: HashMap::new() },
            PlacementNode { id: "n2".into(), worker_id: "w2".into(), config: HashMap::new() },
            PlacementNode { id: "n3".into(), worker_id: "w1".into(), config: HashMap::new() },
        ],
        placement_policy: "cost-optimized-v1".into(),
    };

    let scheduler = DistributedScheduler::new();
    let plan = scheduler.create_plan(&placement_graph);

    assert_eq!(plan.execution_id, "exec_777");
    assert_eq!(plan.execution_order.len(), 3);
    assert_eq!(plan.worker_assignments.len(), 3);
    assert_eq!(plan.worker_assignments.get("n3").unwrap(), "w1");
}
