//! End-to-End Golden Test for Phase 2/3 Repair
//!
//! Tests the full pipeline: Planner → Adapter → Compiler → Scheduler → Runtime
//!
//! Input: "Build a web app" + ExecutionIntent::Balanced
//! Expected: 3-node IR (gen → gen → judge) → 5 passes → DAG schedule → mock provider → success

use std::sync::Arc;
use fusion_compiler::{CompilerEngine, policy};
use fusion_kernel::CapabilitySystem;
use fusion_planner::{ExecutionIntent, PlannerService};
use fusion_runtime::{MockProvider, RuntimeEngine};
use fusion_router::ir::adapter::workflow_to_types;

#[tokio::test]
async fn test_e2e_golden_workflow() {
    // ── Step 1: Planner produces 3-node balanced IR ────────────────────────
    let capability_system = CapabilitySystem::new();
    let planner = PlannerService::new(capability_system);
    let planning_ir = planner.plan_with_intent("Build a web app", ExecutionIntent::Balanced)
        .expect("Planner must produce WorkflowIR");

    let node_count = planning_ir.nodes().len();
    assert_eq!(node_count, 3, "Balanced intent must produce 3 nodes (gen → gen → judge), got {node_count}");

    // ── Step 2: Adapter converts planning IR → execution IR ────────────────
    let exec_ir = workflow_to_types(&planning_ir)
        .expect("Adapter must convert planning IR to execution IR");

    assert_eq!(exec_ir.nodes.len(), 3, "Execution IR must have 3 nodes");

    // ── Step 3: Compiler runs 5 passes + lowers to graph ───────────────────
    let compiler = CompilerEngine::new();
    let (report, graph) = compiler.compile_and_lower("Build a web app", &exec_ir).await
        .expect("Compiler must produce report and graph");

    assert_eq!(report.passes_executed.len(), 5, "Must execute exactly 5 compiler passes (constraint, control_flow, dead_node, model, budget)");
    assert!(report.passes_executed.contains(&"constraint_validation".to_string()));
    assert!(report.passes_executed.contains(&"control_flow_validation".to_string()));
    assert!(report.passes_executed.contains(&"dead_node_elimination".to_string()));
    assert!(report.passes_executed.contains(&"model_resolution".to_string()));
    assert!(report.passes_executed.contains(&"budget_optimisation".to_string()));

    assert_eq!(graph.nodes.len(), 3, "Execution graph must have 3 nodes");
    assert!(!graph.graph_id.is_nil(), "Graph must have a valid ID");

    // ── Step 4: Scheduler runs DAG with mock provider ──────────────────────
    let provider: Arc<dyn fusion_runtime::ChatProvider> = Arc::new(MockProvider::default_response());
    let runtime = RuntimeEngine::new(provider);
    let outcome = runtime.run(Arc::new(graph)).await
        .expect("Runtime must execute graph");

    // ── Step 5: Assert golden workflow success ──────────────────────────────
    assert!(outcome.success, "Golden workflow must succeed");
    assert_eq!(outcome.outputs.len(), 3, "All 3 nodes must produce outputs");
    assert!(outcome.total_tokens > 0, "Must report token usage");
    assert!(outcome.total_latency_ms >= 0, "Must report latency");
}

#[tokio::test]
async fn test_e2e_golden_speed_workflow() {
    // Speed intent → 2-node IR (gen → output)
    let capability_system = CapabilitySystem::new();
    let planner = PlannerService::new(capability_system);
    let planning_ir = planner.plan_with_intent("Quick fix", ExecutionIntent::Speed)
        .expect("Planner must produce WorkflowIR");

    assert_eq!(planning_ir.nodes().len(), 2, "Speed intent must produce 2 nodes");

    let exec_ir = workflow_to_types(&planning_ir).expect("Adapter");
    let compiler = CompilerEngine::new();
    let (_report, graph) = compiler.compile_and_lower("Quick fix", &exec_ir).await
        .expect("Compiler");

    assert_eq!(graph.nodes.len(), 2);

    let provider: Arc<dyn fusion_runtime::ChatProvider> = Arc::new(MockProvider::default_response());
    let runtime = RuntimeEngine::new(provider);
    let outcome = runtime.run(Arc::new(graph)).await.expect("Runtime");

    assert!(outcome.success);
    assert_eq!(outcome.outputs.len(), 2);
}

#[tokio::test]
async fn test_e2e_golden_quality_workflow() {
    // Quality intent → 5-node IR (gen → gen → gen → review → reflection)
    let capability_system = CapabilitySystem::new();
    let planner = PlannerService::new(capability_system);
    let planning_ir = planner.plan_with_intent("Build a web app", ExecutionIntent::Quality)
        .expect("Planner must produce WorkflowIR");

    assert_eq!(planning_ir.nodes().len(), 5, "Quality intent must produce 5 nodes");

    let exec_ir = workflow_to_types(&planning_ir).expect("Adapter");
    let compiler = CompilerEngine::new();
    let (_report, graph) = compiler.compile_and_lower("Build a web app", &exec_ir).await
        .expect("Compiler");

    assert_eq!(graph.nodes.len(), 5);

    let provider: Arc<dyn fusion_runtime::ChatProvider> = Arc::new(MockProvider::default_response());
    let runtime = RuntimeEngine::new(provider);
    let outcome = runtime.run(Arc::new(graph)).await.expect("Runtime");

    assert!(outcome.success);
    assert_eq!(outcome.outputs.len(), 5);
}

#[tokio::test]
async fn test_e2e_policy_deny_blocks_compilation() {
    // Policy deny on shell.exec → compiler rejects the workflow
    let capability_system = CapabilitySystem::new();
    let planner = PlannerService::new(capability_system);
    let planning_ir = planner.plan_with_intent("Build a web app", ExecutionIntent::Balanced)
        .expect("Planner");

    let mut exec_ir = workflow_to_types(&planning_ir).expect("Adapter");
    // Tag first node with a capability that will be denied
    exec_ir.nodes[0].config.insert(
        "capability".into(),
        serde_json::json!("shell.exec"),
    );

    let policy_ir = policy::PolicyIR {
        rules: vec![policy::PolicyRule {
            rule_id: "deny-shell".into(),
            target_pattern: "shell.exec".into(),
            priority: 100,
            effect: policy::PolicyEffect::Deny,
            conditions: vec![],
            actions: vec![],
        }],
    };

    let rm: Arc<dyn fusion_kernel::resource::ResourceManager> =
        Arc::new(fusion_kernel::resource::StubResourceManager::new(f64::INFINITY, u64::MAX));
    let engine = fusion_compiler::build_compiler(
        fusion_types::ModelCatalog::default(),
        rm,
        Some(policy_ir),
    );
    let result = engine.compile("test deny", &exec_ir).await;
    assert!(result.is_err(), "deny policy should block compilation");
}
