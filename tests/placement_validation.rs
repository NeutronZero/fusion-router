//! Placement Engine Operational Validation Suite (v0.14.4)
//!
//! Validates placement metrics, decision latency, GPU affinity, and deterministic placement.

use fusion_core::ExecutionId;
use fusion_ir::WorkflowBuilder;
use fusion_placement::{PlacementEngine, PlacementGraph, PlacementReport};

#[test]
fn test_operational_placement_decision_latency_under_5ms() {
    let engine = PlacementEngine::default();
    let exec_id = ExecutionId::new();
    let ir = WorkflowBuilder::new()
        .task("n1", "CodeGeneration")
        .unwrap()
        .output("n2")
        .unwrap()
        .sequential("n1", "n2")
        .unwrap()
        .build()
        .unwrap();

    let start = std::time::Instant::now();
    let (_graph, report): (PlacementGraph, PlacementReport) = engine.place(&exec_id, &ir).expect("Placement");
    let duration = start.elapsed();

    assert!(duration.as_millis() < 5, "Placement decision latency must be < 5ms");
    assert_eq!(report.node_decisions.len(), 2);
}

#[test]
fn test_operational_placement_gpu_and_locality_scoring() {
    let engine = PlacementEngine::new("gpu-affinity-v1");
    let exec_id = ExecutionId::new();
    let ir = WorkflowBuilder::new()
        .task("n1", "DeepLearningInference")
        .unwrap()
        .build()
        .unwrap();

    let (_graph, report) = engine.place(&exec_id, &ir).expect("Placement");
    assert!(report.node_decisions[0].capability_score >= 0.90);
    assert_eq!(report.placement_policy, "gpu-affinity-v1");
}

#[test]
fn test_operational_placement_determinism_across_runs() {
    let engine = PlacementEngine::default();
    let exec_id = ExecutionId::new();
    let ir = WorkflowBuilder::new()
        .task("n1", "CodeGeneration")
        .unwrap()
        .output("n2")
        .unwrap()
        .sequential("n1", "n2")
        .unwrap()
        .build()
        .unwrap();

    let (g1, r1) = engine.place(&exec_id, &ir).expect("Run 1");
    let (g2, r2) = engine.place(&exec_id, &ir).expect("Run 2");

    assert_eq!(g1.nodes.len(), g2.nodes.len());
    assert_eq!(r1.node_decisions[0].target_worker_id, r2.node_decisions[0].target_worker_id);
}
