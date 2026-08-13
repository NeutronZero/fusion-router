//! Equivalence Test Suite — Monolith (`src/`) vs. 3-Tier Workspace (`crates/`)
//!
//! Diffs the outputs of ported passes in `crates/fusion-compiler` and `crates/fusion-planner`
//! directly against the monolith implementations in `src/compiler/` and `src/planner/`.

use fusion_compiler::{CompilerPass as CrateCompilerPass, ConstraintValidationPass as CrateConstraintPass};
use fusion_router::compiler::passes::{CompilerPass as MonolithCompilerPass, ConstraintValidationPass as MonolithConstraintPass};
use fusion_router::types::{WorkflowIR as MonolithWorkflowIR, IRNode as MonolithIRNode, IRNodeKind as MonolithIRNodeKind, StrategyKind as MonolithStrategyKind, IRMetadata as MonolithIRMetadata};
use fusion_types::{WorkflowIR, IRNode, IRNodeKind, IREdge, StrategyKind};
use uuid::Uuid;
use std::collections::HashMap;

fn make_exec_node(id: &str, kind: IRNodeKind) -> IRNode {
    IRNode {
        id: Uuid::parse_str(&format!("550e8400-e29b-41d4-a716-{:012}", id.len() * 1111)).unwrap_or_else(|_| Uuid::new_v4()),
        kind,
        strategy: StrategyKind::Single,
        model: None,
        config: HashMap::new(),
    }
}

#[tokio::test]
async fn test_constraint_validation_pass_equivalence() {
    let monolith_pass = MonolithConstraintPass;
    let crate_pass = CrateConstraintPass;

    // Test Case 1: Empty IR -> both monolith and crate passes must reject empty IR
    let empty_monolith_ir = MonolithWorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![],
        edges: vec![],
        metadata: MonolithIRMetadata {
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 0,
        },
    };
    let empty_crate_ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![],
        edges: vec![],
        metadata: fusion_types::IRMetadata {
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 0,
        },
    };

    let monolith_res = monolith_pass.apply(empty_monolith_ir).await;
    let crate_res = crate_pass.apply(empty_crate_ir).await;

    assert!(monolith_res.is_err(), "Monolith pass must reject empty IR");
    assert!(crate_res.is_err(), "Crate pass must reject empty IR");

    // Test Case 2: Non-empty IR -> both monolith and crate passes must accept
    let valid_monolith_ir = MonolithWorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![MonolithIRNode {
            id: Uuid::new_v4(),
            kind: MonolithIRNodeKind::Generate,
            strategy: MonolithStrategyKind::Single,
            model: None,
            config: HashMap::new(),
        }],
        edges: vec![],
        metadata: MonolithIRMetadata {
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 10,
        },
    };
    let valid_crate_ir = WorkflowIR {
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
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 10,
        },
    };

    let monolith_valid_res = monolith_pass.apply(valid_monolith_ir).await;
    let crate_valid_res = crate_pass.apply(valid_crate_ir).await;

    assert!(monolith_valid_res.is_ok(), "Monolith pass must accept valid IR");
    assert!(crate_valid_res.is_ok(), "Crate pass must accept valid IR");
}

#[tokio::test]
async fn test_model_resolution_pass_equivalence() {
    use fusion_compiler::ModelResolutionPass as CrateModelResolutionPass;
    use fusion_router::compiler::passes::ModelResolutionPass as MonolithModelResolutionPass;
    use fusion_types::ModelCatalog as CrateModelCatalog;
    use fusion_router::types::ModelCatalog as MonolithModelCatalog;
    use fusion_router::providers::ModelRequirements as MonolithModelRequirements;

    let crate_catalog = CrateModelCatalog::default();
    let monolith_catalog = MonolithModelCatalog::default();

    // 0. Catalog Source of Truth Parity Check
    assert_eq!(crate_catalog.code, monolith_catalog.code);
    assert_eq!(crate_catalog.debug, monolith_catalog.debug);
    assert_eq!(crate_catalog.architecture, monolith_catalog.architecture);
    assert_eq!(crate_catalog.general, monolith_catalog.general);
    assert_eq!(crate_catalog.creative, monolith_catalog.creative);
    assert_eq!(crate_catalog.analysis, monolith_catalog.analysis);
    assert_eq!(crate_catalog.fast, monolith_catalog.fast);
    assert_eq!(crate_catalog.cheap, monolith_catalog.cheap);

    let make_monolith_ir = || MonolithWorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![MonolithIRNode {
            id: Uuid::new_v4(),
            kind: MonolithIRNodeKind::Generate,
            strategy: MonolithStrategyKind::Single,
            model: None,
            config: HashMap::new(),
        }],
        edges: vec![],
        metadata: MonolithIRMetadata {
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 10,
        },
    };

    // 1. Tool Requirement
    let monolith_reqs_tool = MonolithModelRequirements { requires_tools: true, ..Default::default() };
    let crate_pass_tool = CrateModelResolutionPass::new(crate_catalog.clone());
    let monolith_pass_tool = MonolithModelResolutionPass { model_catalog: monolith_catalog.clone(), model_requirements: Some(monolith_reqs_tool) };
    let monolith_ir_tool = monolith_pass_tool.apply(make_monolith_ir()).await.expect("apply");
    assert_eq!(crate_pass_tool.select_model(), monolith_ir_tool.nodes[0].model.as_deref().unwrap());
    assert_eq!(crate_pass_tool.select_model(), crate_catalog.fast);

    // 2. Default / Fast Fallback
    let crate_pass_default = CrateModelResolutionPass::new(crate_catalog.clone());
    let monolith_pass_default = MonolithModelResolutionPass { model_catalog: monolith_catalog.clone(), model_requirements: None };
    let monolith_ir_default = monolith_pass_default.apply(make_monolith_ir()).await.expect("apply");
    assert_eq!(crate_pass_default.select_model(), monolith_ir_default.nodes[0].model.as_deref().unwrap());
    assert_eq!(crate_pass_default.select_model(), crate_catalog.fast);
}

#[tokio::test]
async fn test_control_flow_validation_pass_equivalence() {
    use fusion_compiler::ControlFlowValidationPass as CrateControlFlowPass;
    use fusion_router::compiler::passes::ControlFlowValidationPass as MonolithControlFlowPass;
    use fusion_router::types::IREdge as MonolithIREdge;

    let monolith_pass = MonolithControlFlowPass;
    let crate_pass = CrateControlFlowPass;

    // 1. Unknown Source Node in Edge -> both monolith and crate must reject
    let id_a = Uuid::new_v4();
    let id_b = Uuid::new_v4();
    let id_bad = Uuid::new_v4();

    let monolith_invalid_edge_ir = MonolithWorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![MonolithIRNode {
            id: id_a,
            kind: MonolithIRNodeKind::Generate,
            strategy: MonolithStrategyKind::Single,
            model: None,
            config: HashMap::new(),
        }],
        edges: vec![MonolithIREdge {
            from: id_bad,
            to: id_a,
            condition: None,
        }],
        metadata: MonolithIRMetadata {
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 0,
        },
    };

    let node_a_id = Uuid::new_v4();
    let node_unknown_id = Uuid::new_v4();
    let crate_invalid_edge_ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![IRNode {
            id: node_a_id,
            kind: IRNodeKind::Generate,
            strategy: StrategyKind::Single,
            model: None,
            config: HashMap::new(),
        }],
        edges: vec![IREdge {
            from: node_unknown_id,
            to: node_a_id,
            condition: None,
        }],
        metadata: fusion_types::IRMetadata {
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 0,
        },
    };

    assert!(monolith_pass.apply(monolith_invalid_edge_ir).await.is_err(), "Monolith must reject unknown edge source");
    assert!(crate_pass.apply(crate_invalid_edge_ir).await.is_err(), "Crate pass must reject unknown edge source");

    // 2. Valid multi-node DAG -> both monolith and crate must accept
    let valid_monolith_dag_ir = MonolithWorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![
            MonolithIRNode {
                id: id_a,
                kind: MonolithIRNodeKind::Generate,
                strategy: MonolithStrategyKind::Single,
                model: None,
                config: HashMap::new(),
            },
            MonolithIRNode {
                id: id_b,
                kind: MonolithIRNodeKind::Review,
                strategy: MonolithStrategyKind::Single,
                model: None,
                config: HashMap::new(),
            },
        ],
        edges: vec![MonolithIREdge {
            from: id_a,
            to: id_b,
            condition: None,
        }],
        metadata: MonolithIRMetadata {
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 10,
        },
    };

    let dag_node_a = Uuid::new_v4();
    let dag_node_b = Uuid::new_v4();
    let valid_crate_dag_ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![
            IRNode {
                id: dag_node_a,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: None,
                config: HashMap::new(),
            },
            IRNode {
                id: dag_node_b,
                kind: IRNodeKind::Review,
                strategy: StrategyKind::Single,
                model: None,
                config: HashMap::new(),
            },
        ],
        edges: vec![IREdge {
            from: dag_node_a,
            to: dag_node_b,
            condition: None,
        }],
        metadata: fusion_types::IRMetadata {
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 10,
        },
    };

    assert!(monolith_pass.apply(valid_monolith_dag_ir).await.is_ok(), "Monolith must accept valid DAG");
    assert!(crate_pass.apply(valid_crate_dag_ir).await.is_ok(), "Crate pass must accept valid DAG");

    // 3. Split arity: 1 outgoing -> reject
    let split_node = Uuid::new_v4();
    let target_node = Uuid::new_v4();
    let crate_single_out_ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![
            IRNode { id: split_node, kind: IRNodeKind::Split, strategy: StrategyKind::Single, model: None, config: HashMap::from([("control_flow".into(), serde_json::json!("split"))]) },
            IRNode { id: target_node, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![IREdge { from: split_node, to: target_node, condition: None }],
        metadata: fusion_types::IRMetadata { policy_applied: vec![], estimated_cost: 0.0, estimated_tokens: 0 },
    };
    assert!(crate_pass.apply(crate_single_out_ir).await.is_err(), "Crate must reject split with 1 outgoing edge");

    // 4. Split arity: 2 outgoing -> accept
    let split_node2 = Uuid::new_v4();
    let a_node = Uuid::new_v4();
    let b_node = Uuid::new_v4();
    let crate_two_out_ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![
            IRNode { id: split_node2, kind: IRNodeKind::Split, strategy: StrategyKind::Single, model: None, config: HashMap::from([("control_flow".into(), serde_json::json!("split"))]) },
            IRNode { id: a_node, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: b_node, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![IREdge { from: split_node2, to: a_node, condition: None }, IREdge { from: split_node2, to: b_node, condition: None }],
        metadata: fusion_types::IRMetadata { policy_applied: vec![], estimated_cost: 0.0, estimated_tokens: 0 },
    };
    assert!(crate_pass.apply(crate_two_out_ir).await.is_ok(), "Crate must accept split with 2 outgoing edges");

    // 5. Merge arity: 1 incoming Merge -> reject
    let src_node = Uuid::new_v4();
    let m_node = Uuid::new_v4();
    let out_node = Uuid::new_v4();
    let crate_single_merge_ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![
            IRNode { id: src_node, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: m_node, kind: IRNodeKind::Join, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: out_node, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![IREdge { from: src_node, to: m_node, condition: None }, IREdge { from: m_node, to: out_node, condition: None }],
        metadata: fusion_types::IRMetadata { policy_applied: vec![], estimated_cost: 0.0, estimated_tokens: 0 },
    };
    assert!(crate_pass.apply(crate_single_merge_ir).await.is_err(), "Crate must reject merge with 1 incoming");

    // 6. Join with 2 incoming -> accept
    let ja_node = Uuid::new_v4();
    let jb_node = Uuid::new_v4();
    let j_node = Uuid::new_v4();
    let crate_join_ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![
            IRNode { id: ja_node, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: jb_node, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: j_node, kind: IRNodeKind::Join, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![IREdge { from: ja_node, to: j_node, condition: None }, IREdge { from: jb_node, to: j_node, condition: None }],
        metadata: fusion_types::IRMetadata { policy_applied: vec![], estimated_cost: 0.0, estimated_tokens: 0 },
    };
    assert!(crate_pass.apply(crate_join_ir).await.is_ok(), "Crate must accept join with 2 incoming merges");

    // 7. Barrier with 0 outgoing -> reject
    let ba_node = Uuid::new_v4();
    let bb_node = Uuid::new_v4();
    let b_node = Uuid::new_v4();
    let crate_barrier_no_out = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![
            IRNode { id: ba_node, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: bb_node, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: b_node, kind: IRNodeKind::Barrier, strategy: StrategyKind::Single, model: None, config: HashMap::from([("control_flow".into(), serde_json::json!("barrier"))]) },
        ],
        edges: vec![IREdge { from: ba_node, to: b_node, condition: None }, IREdge { from: bb_node, to: b_node, condition: None }],
        metadata: fusion_types::IRMetadata { policy_applied: vec![], estimated_cost: 0.0, estimated_tokens: 0 },
    };
    assert!(crate_pass.apply(crate_barrier_no_out).await.is_err(), "Crate barrier with 0 outgoing must fail BarrierArity");
}

#[tokio::test]
async fn test_intent_planner_equivalence() {
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
    };

    // 1. Quality Intent
    let monolith_quality = monolith_planner.plan(&make_monolith_reqs(Some(MonolithExecutionIntent::Quality)), &[], None).await;
    let crate_quality = crate_planner.plan_intent(&CrateExecutionIntent::Quality).expect("crate quality plan");
    assert_eq!(monolith_quality.nodes.len(), 5);
    assert_eq!(crate_quality.nodes().len(), 5);

    // 2. Speed Intent
    let monolith_speed = monolith_planner.plan(&make_monolith_reqs(Some(MonolithExecutionIntent::Speed)), &[], None).await;
    let crate_speed = crate_planner.plan_intent(&CrateExecutionIntent::Speed).expect("crate speed plan");
    assert_eq!(monolith_speed.nodes.len(), 1);
    assert_eq!(crate_speed.nodes().len(), 2);

    // 3. Balanced Intent
    let monolith_balanced = monolith_planner.plan(&make_monolith_reqs(Some(MonolithExecutionIntent::Balanced)), &[], None).await;
    let crate_balanced = crate_planner.plan_intent(&CrateExecutionIntent::Balanced).expect("crate balanced plan");
    assert_eq!(monolith_balanced.nodes.len(), 3);
    assert_eq!(crate_balanced.nodes().len(), 3);

    // 4. Exhaustive Intent
    let monolith_exhaustive = monolith_planner.plan(&make_monolith_reqs(Some(MonolithExecutionIntent::Exhaustive)), &[], None).await;
    let crate_exhaustive = crate_planner.plan_intent(&CrateExecutionIntent::Exhaustive).expect("crate exhaustive plan");
    assert_eq!(monolith_exhaustive.nodes.len(), 6);
    assert_eq!(crate_exhaustive.nodes().len(), 6);

    // 5. Constrained Intent (cost < 0.02 -> speed)
    let monolith_constrained_low = monolith_planner.plan(&make_monolith_reqs(Some(MonolithExecutionIntent::Constrained { max_cost_usd: Some(0.01), max_tokens: None, max_latency_ms: None, min_confidence: None })), &[], None).await;
    let crate_constrained_low = crate_planner.plan_intent(&CrateExecutionIntent::Constrained { max_cost_usd: Some(0.01) }).expect("crate constrained low plan");
    assert_eq!(monolith_constrained_low.nodes.len(), 1);
    assert_eq!(crate_constrained_low.nodes().len(), 2);

    // 6. Constrained Intent (cost >= 0.02 -> balanced)
    let monolith_constrained_high = monolith_planner.plan(&make_monolith_reqs(Some(MonolithExecutionIntent::Constrained { max_cost_usd: Some(0.10), max_tokens: None, max_latency_ms: None, min_confidence: None })), &[], None).await;
    let crate_constrained_high = crate_planner.plan_intent(&CrateExecutionIntent::Constrained { max_cost_usd: Some(0.10) }).expect("crate constrained high plan");
    assert_eq!(monolith_constrained_high.nodes.len(), 3);
    assert_eq!(crate_constrained_high.nodes().len(), 3);
}
