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
use fusion_types::{IRNode, IRNodeKind, StrategyKind, IREdge};
use std::collections::HashMap;

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

    assert_eq!(report.passes_executed.len(), 5, "Must execute exactly 5 compiler passes");
    assert_eq!(report.pass_diffs.len(), 5);
    for diff in &report.pass_diffs {
        assert!(diff.duration_ms >= 0, "pass {} timing must be non-negative", diff.pass_name);
    }

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
    let capability_system = CapabilitySystem::new();
    let planner = PlannerService::new(capability_system);
    let planning_ir = planner.plan_with_intent("Quick fix", ExecutionIntent::Speed)
        .expect("Planner must produce WorkflowIR");

    assert_eq!(planning_ir.nodes().len(), 1, "Speed intent must produce 1 node");

    let exec_ir = workflow_to_types(&planning_ir).expect("Adapter");
    let compiler = CompilerEngine::new();
    let (report, graph) = compiler.compile_and_lower("Quick fix", &exec_ir).await
        .expect("Compiler");

    assert_eq!(report.passes_executed.len(), 5);
    assert_eq!(graph.nodes.len(), 1);

    let provider: Arc<dyn fusion_runtime::ChatProvider> = Arc::new(MockProvider::default_response());
    let runtime = RuntimeEngine::new(provider);
    let outcome = runtime.run(Arc::new(graph)).await.expect("Runtime");

    assert!(outcome.success);
    assert_eq!(outcome.outputs.len(), 1);
}

#[tokio::test]
async fn test_e2e_golden_quality_workflow() {
    let capability_system = CapabilitySystem::new();
    let planner = PlannerService::new(capability_system);
    let planning_ir = planner.plan_with_intent("Build a web app", ExecutionIntent::Quality)
        .expect("Planner must produce WorkflowIR");

    assert_eq!(planning_ir.nodes().len(), 5, "Quality intent must produce 5 nodes");

    let exec_ir = workflow_to_types(&planning_ir).expect("Adapter");
    let compiler = CompilerEngine::new();
    let (report, graph) = compiler.compile_and_lower("Build a web app", &exec_ir).await
        .expect("Compiler");

    assert_eq!(report.passes_executed.len(), 5);
    assert_eq!(graph.nodes.len(), 5);

    let provider: Arc<dyn fusion_runtime::ChatProvider> = Arc::new(MockProvider::default_response());
    let runtime = RuntimeEngine::new(provider);
    let outcome = runtime.run(Arc::new(graph)).await.expect("Runtime");

    assert!(outcome.success);
    assert_eq!(outcome.outputs.len(), 5);
}

#[tokio::test]
async fn test_e2e_dead_node_elimination() {
    // Build a 3-node IR (Balanced) and inject an orphan node D with no edges.
    // After compilation, graph must contain only the 3 live nodes.
    let capability_system = CapabilitySystem::new();
    let planner = PlannerService::new(capability_system);
    let planning_ir = planner.plan_with_intent("Quick fix", ExecutionIntent::Balanced)
        .expect("Planner");
    let mut exec_ir = workflow_to_types(&planning_ir).expect("Adapter");

    // Inject orphan node (no incoming or outgoing edges)
    let orphan_id = uuid::Uuid::new_v4();
    exec_ir.nodes.push(IRNode {
        id: orphan_id,
        kind: IRNodeKind::Generate,
        strategy: StrategyKind::Single,
        model: None,
        config: HashMap::new(),
    });
    assert_eq!(exec_ir.nodes.len(), 4, "IR must have 4 nodes (3 live + 1 orphan)");

    let compiler = CompilerEngine::new();
    let (_report, graph) = compiler.compile_and_lower("dead node test", &exec_ir).await
        .expect("Compiler");

    assert_eq!(graph.nodes.len(), 3, "orphan must be eliminated from graph");
    assert!(!graph.nodes.iter().any(|n| n.id == orphan_id),
        "orphan node must not appear in execution graph");
}

#[tokio::test]
async fn test_e2e_policy_deny_blocks_compilation() {
    let capability_system = CapabilitySystem::new();
    let planner = PlannerService::new(capability_system);
    let planning_ir = planner.plan_with_intent("Build a web app", ExecutionIntent::Balanced)
        .expect("Planner");

    let mut exec_ir = workflow_to_types(&planning_ir).expect("Adapter");
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
        Arc::new(fusion_kernel::resource::StubResourceManager::new(fusion_kernel::resource::Quota { max_daily_cost: fusion_core::NanoUSD::from_nanos(u64::MAX), max_daily_tokens: u64::MAX }));
    let engine = fusion_compiler::build_compiler(
        fusion_types::ModelCatalog::default(),
        rm,
        Some(policy_ir),
    );
    let result = engine.compile("test deny", &exec_ir).await;
    assert!(result.is_err(), "deny policy should block compilation");
}
