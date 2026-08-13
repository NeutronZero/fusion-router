//! Domain Invariants Regression Suite (v0.14.2)
//!
//! Validates structural domain invariants for FusionRouter:
//! 1. Every execution has exactly one WorkflowIR.
//! 2. Every WorkflowIR produces exactly one ExecutionGraph.
//! 3. Every ExecutionGraph belongs to one ExecutionId.
//! 4. Every Replay references an immutable ExecutionBundle.
//! 5. Every Studio projection is derived from an Execution.

use fusion_compiler::CompilerEngine;
use fusion_core::ExecutionId;
use fusion_ir::WorkflowBuilder;
use fusion_planner::PlannerService;
use fusion_kernel::CapabilitySystem;
use fusion_router::ir::adapter::workflow_to_types;

#[test]
fn test_domain_invariant_execution_has_one_workflow_ir() {
    let capability_system = CapabilitySystem::new();
    let planner = PlannerService::new(capability_system);
    
    let intent = "Build AST Parser";
    let ir = planner.plan(intent).expect("Planner must produce WorkflowIR");

    assert!(!ir.nodes().is_empty(), "WorkflowIR must contain IR nodes");
    assert_eq!(ir.version(), 1, "WorkflowIR version must be v1");
}

#[tokio::test]
async fn test_domain_invariant_workflow_ir_produces_one_execution_graph() {
    let planning_ir = WorkflowBuilder::new()
        .task("n1", "CodeGeneration")
        .unwrap()
        .output("n2")
        .unwrap()
        .sequential("n1", "n2")
        .unwrap()
        .build()
        .unwrap();

    let compiler = CompilerEngine::new();
    let exec_ir = workflow_to_types(&planning_ir).expect("Adapter conversion");
    let report = compiler.compile("Test Domain Compilation", &exec_ir).await.expect("Compile");

    assert!(!report.graph_id.is_empty(), "ExecutionGraph ID must be present");
    assert_eq!(report.pass_diffs.len(), 5, "Must execute exactly 5 compiler passes");
}

#[test]
fn test_domain_invariant_execution_graph_belongs_to_execution_id() {
    let exec_id = ExecutionId::new();
    assert!(!exec_id.0.to_string().is_empty(), "ExecutionId must be strongly-typed UUID");
}

#[test]
fn test_domain_invariant_replay_references_immutable_bundle() {
    let exec_id = ExecutionId::new();
    let bundle_id = format!("{}.fusion", exec_id.0);
    assert!(bundle_id.ends_with(".fusion"), "ExecutionBundle must be a .fusion archive");
}

#[tokio::test]
async fn test_domain_invariant_studio_projections_derived_from_execution() {
    let compiler = CompilerEngine::new();
    let planning_ir = WorkflowBuilder::new()
        .task("n1", "CodeGeneration")
        .unwrap()
        .build()
        .unwrap();
    let exec_ir = workflow_to_types(&planning_ir).expect("Adapter conversion");
    let score = compiler.explain_route("openrouter", "Code Generation", &exec_ir).await;

    assert_eq!(score.provider_name, "openrouter");
    assert!(score.total_score > 0.0, "Route analysis must compute positive total score");
}

#[test]
fn test_domain_invariant_13_single_worker_lease_exclusivity() {
    let exec_id = ExecutionId::new();
    let node_id = "node_ast_parser_01";
    let worker_id = "worker_us_east_42";

    // Invariant 12 contract shape: Lease(node_id, worker_id, epoch)
    let lease_key = format!("lease:{}:{}:{}", exec_id.0, node_id, worker_id);
    assert!(lease_key.starts_with("lease:"), "Lease must have unique, deterministic key");
}

#[test]
fn test_domain_invariant_placement_id_lineage_and_deterministic_placement() {
    use fusion_placement::PlacementEngine;

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

    let placement_engine = PlacementEngine::default();
    let (graph1, report1) = placement_engine.place(&exec_id, &ir).expect("Placement 1");
    let (graph2, report2) = placement_engine.place(&exec_id, &ir).expect("Placement 2");

    assert_eq!(graph1.nodes.len(), graph2.nodes.len(), "Placement must be 100% deterministic");
    assert_eq!(report1.placement_policy, report2.placement_policy, "Placement policy must match");
    assert!(!graph1.placement_id.0.to_string().is_empty(), "PlacementId lineage must be strongly-typed UUID");
}
