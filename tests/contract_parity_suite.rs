//! Contract Parity & Invariant Behavior Test Suite
//!
//! Purpose:
//! 1. Verify host delegation shims in `src/` faithfully bridge to `crates/` without behavioral drift.
//! 2. Exercise runtime/compiler invariant behavior under deterministic unit testing.

use uuid::Uuid;
use std::collections::HashMap;
use fusion_types::{WorkflowIR, IRNode, IRNodeKind, IREdge, StrategyKind, NanoUSD};

// ---------------------------------------------------------------------------
// 1. Host Intent Planner Delegation Parity
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_intent_planner_host_shim_delegation_parity() {
    use fusion_planner::{ExecutionIntent as CrateExecutionIntent, IntentPlanner as CrateIntentPlanner};
    use fusion_router::planner::IntentPlanner as MonolithIntentPlanner;
    use fusion_router::types::execution::ExecutionIntent as MonolithExecutionIntent;
    use fusion_router::types::{Requirements, Intent, ComplexityLevel, ModelCatalog as MonolithModelCatalog};
    use fusion_core::ModelCatalog as CrateModelCatalog;
    use fusion_router::planner::Planner;

    let monolith_planner = MonolithIntentPlanner::new(MonolithModelCatalog::default());
    let crate_planner = CrateIntentPlanner::new(CrateModelCatalog::default());

    let make_monolith_reqs = |intent: Option<MonolithExecutionIntent>| Requirements {
        intent_classification: Intent::General,
        complexity: ComplexityLevel::Medium,
        has_files: false,
        context_window: 4096,
        original_text: "test intent".to_string(),
        execution_intent: intent,
        output_preferences: None,
        model_requirements: None,
        requested_model: None,
        requested_strategy: None,
    };

    let make_crate_req = |intent: CrateExecutionIntent| fusion_planner::PlanningRequest {
        intent,
        user_prompt: "test intent".to_string(),
        requested_model: None,
        requested_strategy: None,
        strategy_config: None,
        requirements: fusion_planner::RequirementsSnapshot::default(),
        policies: fusion_planner::PolicySnapshot::default(),
        capability_catalog: fusion_planner::CapabilityCatalogSnapshot::default(),
        model_catalog: fusion_planner::ModelCatalogSnapshot::new(CrateModelCatalog::default()),
        telemetry: fusion_planner::RoutingTelemetrySnapshot::default(),
    };

    // Quality Intent Parity
    let monolith_quality = monolith_planner.plan(&make_monolith_reqs(Some(MonolithExecutionIntent::Quality)), &[], None).await;
    let crate_quality = crate_planner.plan(&make_crate_req(CrateExecutionIntent::Quality)).expect("crate quality plan");
    assert_eq!(monolith_quality.nodes.len(), 5);
    assert_eq!(crate_quality.nodes().len(), 5);

    // Speed Intent Parity
    let monolith_speed = monolith_planner.plan(&make_monolith_reqs(Some(MonolithExecutionIntent::Speed)), &[], None).await;
    let crate_speed = crate_planner.plan(&make_crate_req(CrateExecutionIntent::Speed)).expect("crate speed plan");
    assert_eq!(monolith_speed.nodes.len(), 1);
    assert_eq!(crate_speed.nodes().len(), 1);

    // Balanced Intent Parity
    let monolith_balanced = monolith_planner.plan(&make_monolith_reqs(Some(MonolithExecutionIntent::Balanced)), &[], None).await;
    let crate_balanced = crate_planner.plan(&make_crate_req(CrateExecutionIntent::Balanced)).expect("crate balanced plan");
    assert_eq!(monolith_balanced.nodes.len(), 3);
    assert_eq!(crate_balanced.nodes().len(), 3);
}

// ---------------------------------------------------------------------------
// 2. Policy Control Plane & Snapshot Parity
// ---------------------------------------------------------------------------

#[test]
fn test_policy_registry_snapshot_and_evaluation_parity() {
    use fusion_router::policy::PolicyRegistry;

    let registry = PolicyRegistry::new();
    assert_eq!(registry.current_snapshot().version, 1);
    assert_eq!(registry.policy_count(), 0);

    // Apply policy mutation (version becomes 2)
    let snapshot_v2 = registry.apply_policy(
        "pol-1".into(),
        "deny-unauth".into(),
        "match admin.* => deny".into(),
    );

    assert_eq!(snapshot_v2.version, 2);
    assert_eq!(snapshot_v2.policies.len(), 1);
    assert_eq!(snapshot_v2.policies[0].id, "pol-1");
    assert_eq!(snapshot_v2.policies[0].name, "deny-unauth");

    // Historical version lookup
    let historical = registry.snapshot_at(1).expect("Version 1 must exist");
    assert_eq!(historical.version, 1);
    assert_eq!(historical.policies.len(), 0);

    // Current snapshot matches v2
    let current = registry.current_snapshot();
    assert_eq!(current.version, 2);
    assert_eq!(current.policies.len(), 1);
}

// ---------------------------------------------------------------------------
// 3. Compiler Adapter Forwarding Smoke Test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_compiler_adapter_forwarding_smoke_test() {
    use fusion_router::compiler::build_compiler;
    use fusion_router::resource::DefaultResourceManager;
    use fusion_router::types::{ModelCatalog, Quota};
    use std::sync::Arc;

    let quota = Quota {
        max_daily_cost: NanoUSD::from_nanos(1_000_000_000_000),
        max_daily_tokens: 1_000_000_000,
        max_concurrent: 10,
        provider_limits: HashMap::new(),
    };
    let rm = Arc::new(DefaultResourceManager::new(quota));
    let compiler = build_compiler(ModelCatalog::default(), rm, None);

    let ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![IRNode {
            id: Uuid::new_v4(),
            kind: IRNodeKind::Generate,
            strategy: StrategyKind::Single,
            model: None,
            config: HashMap::new(),
        }],
        edges: vec![],
        metadata: fusion_types::IRMetadata {
            policy_version: 0,
            policy_applied: vec![],
            estimated_cost: NanoUSD::from_nanos(10_000_000),
            estimated_tokens: 100,
        },
    };

    use fusion_router::compiler::Compiler;
    let graph = compiler.compile(ir).await.expect("compile through host adapter");
    assert_eq!(graph.nodes.len(), 1);
}

// ---------------------------------------------------------------------------
// 4. Invariant & Crate Behavior Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_dead_node_elimination_reachability_behavior() {
    use fusion_compiler::{CompilerPass, DeadNodeEliminationPass};

    let pass = DeadNodeEliminationPass;

    let n_root = Uuid::new_v4();
    let n_child = Uuid::new_v4();
    let n_orphan = Uuid::new_v4();
    let orphan_ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![
            IRNode { id: n_root, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: Some("gpt-4o".into()), config: HashMap::new() },
            IRNode { id: n_child, kind: IRNodeKind::Review, strategy: StrategyKind::Single, model: Some("claude-3-5".into()), config: HashMap::new() },
            IRNode { id: n_orphan, kind: IRNodeKind::Transform, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![IREdge { from: n_root, to: n_child, condition: None }],
        metadata: fusion_types::IRMetadata {
            policy_version: 0,
            policy_applied: vec![],
            estimated_cost: NanoUSD::ZERO,
            estimated_tokens: 200,
        },
    };
    let out = pass.apply(orphan_ir).await.expect("apply orphan pass");
    assert_eq!(out.nodes.len(), 2, "Orphaned node must be eliminated");
    assert!(!out.nodes.iter().any(|n| n.id == n_orphan));
}

#[tokio::test]
async fn test_budget_optimisation_affordability_behavior() {
    use fusion_compiler::{CompilerPass, BudgetOptimisationPass};
    use fusion_kernel::resource::{StubResourceManager, Quota};
    use std::sync::Arc;

    let rm = Arc::new(StubResourceManager::new(Quota {
        max_daily_cost: NanoUSD::from_nanos(100_000_000_000),
        max_daily_tokens: 1_000_000,
    }));
    let pass = BudgetOptimisationPass { resource_manager: rm };
    let ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![IRNode { id: Uuid::new_v4(), kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() }],
        edges: vec![],
        metadata: fusion_types::IRMetadata {
            policy_version: 0,
            policy_applied: vec![],
            estimated_cost: NanoUSD::from_nanos(10_000_000),
            estimated_tokens: 500,
        },
    };
    assert!(pass.apply(ir).await.is_ok());
}

#[test]
fn test_budget_envelope_spend_accumulation_behavior() {
    use fusion_types::{BudgetEnvelope, BudgetExceededError};

    let envelope = BudgetEnvelope::new(NanoUSD::from_nanos(50_000_000), 5_000, 3);
    assert!(envelope.record_and_check(NanoUSD::from_nanos(20_000_000), 2_000).is_ok());
    assert_eq!(envelope.spent_cost().as_nanos(), 20_000_000);

    let cloned = envelope.clone();
    assert!(cloned.record_and_check(NanoUSD::from_nanos(20_000_000), 2_000).is_ok());
    assert_eq!(envelope.spent_cost().as_nanos(), 40_000_000);

    let err = envelope.record_and_check(NanoUSD::from_nanos(20_000_000), 100).unwrap_err();
    assert_eq!(err, BudgetExceededError::Cost { spent: 60_000_000, max: 50_000_000 });
}

#[test]
fn test_strategy_expansion_topological_behavior() {
    use fusion_compiler::strategy_expansion::expanded_subgraph;
    use fusion_types::{ExecutionNode, ExecutionNodeKind, RetryPolicy};

    let node_consensus = ExecutionNode {
        id: Uuid::new_v4(),
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Consensus,
        model: "gpt-4o".into(),
        retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
        fallback: None,
        config: HashMap::from([("members".into(), serde_json::json!(["gpt-4o", "claude-3-5-sonnet"]))]),
        subgraph: None,
    };

    let consensus_subgraph = expanded_subgraph(&node_consensus).expect("expanded consensus subgraph");
    assert_eq!(consensus_subgraph.nodes.len(), 3);
    assert_eq!(consensus_subgraph.edges.len(), 2);
}

#[test]
fn test_capability_system_support_behavior() {
    use fusion_kernel::{CapabilityCatalog, CapabilitySystem};

    let catalog = CapabilityCatalog::new();
    assert!(catalog.supports("Vision"));
    assert!(catalog.supports("MCP"));
    assert!(catalog.supports("ToolCalling"));
    assert!(catalog.supports("Reasoning"));

    let system = CapabilitySystem::new();
    assert!(system.supports("Vision"));
    assert!(system.supports("ToolUse"));
    assert!(system.supports("Reasoning"));
}
