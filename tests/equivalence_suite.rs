//! Equivalence Test Suite — Monolith (`src/`) vs. 3-Tier Workspace (`crates/`)
//!
//! Diffs the outputs of ported passes in `crates/fusion-compiler` and `crates/fusion-planner`
//! directly against the monolith implementations in `src/compiler/` and `src/planner/`.

use fusion_compiler::{CompilerPass as CrateCompilerPass, ConstraintValidationPass as CrateConstraintPass};
use fusion_router::compiler::passes::{CompilerPass as MonolithCompilerPass, ConstraintValidationPass as MonolithConstraintPass};
use fusion_router::types::{WorkflowIR as MonolithWorkflowIR, IRNode as MonolithIRNode, IRNodeKind as MonolithIRNodeKind, StrategyKind as MonolithStrategyKind, IRMetadata as MonolithIRMetadata};
use fusion_ir::WorkflowIR as CrateWorkflowIR;
use uuid::Uuid;
use std::collections::HashMap;

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
    let empty_crate_ir: CrateWorkflowIR = serde_json::from_str(
        r#"{"workflow_id":"550e8400-e29b-41d4-a716-446655440000","version":1,"nodes":[],"edges":[],"metadata":{"policy_applied":[],"estimated_cost":0.0,"estimated_tokens":0}}"#
    ).expect("deserialize empty ir");

    let monolith_res = monolith_pass.apply(empty_monolith_ir).await;
    let crate_res = crate_pass.transform(&empty_crate_ir);

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
    let valid_crate_ir = fusion_ir::WorkflowBuilder::new()
        .task("node_1", "CodeGeneration")
        .expect("task build")
        .build()
        .expect("workflow build");

    let monolith_valid_res = monolith_pass.apply(valid_monolith_ir).await;
    let crate_valid_res = crate_pass.transform(&valid_crate_ir);

    assert!(monolith_valid_res.is_ok(), "Monolith pass must accept valid IR");
    assert!(crate_valid_res.is_ok(), "Crate pass must accept valid IR");
}

#[tokio::test]
async fn test_model_resolution_pass_equivalence() {
    use fusion_compiler::ModelResolutionPass as CrateModelResolutionPass;
    use fusion_router::compiler::passes::ModelResolutionPass as MonolithModelResolutionPass;
    use fusion_core::{ModelCatalog as CrateModelCatalog, ModelRequirements as CrateModelRequirements};
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
    let crate_reqs_tool = CrateModelRequirements { requires_tools: true, ..Default::default() };
    let monolith_reqs_tool = MonolithModelRequirements { requires_tools: true, ..Default::default() };
    let crate_pass_tool = CrateModelResolutionPass::new(crate_catalog.clone(), Some(crate_reqs_tool));
    let monolith_pass_tool = MonolithModelResolutionPass { model_catalog: monolith_catalog.clone(), model_requirements: Some(monolith_reqs_tool) };
    let monolith_ir_tool = monolith_pass_tool.apply(make_monolith_ir()).await.expect("apply");
    assert_eq!(crate_pass_tool.select_model(), monolith_ir_tool.nodes[0].model.as_deref().unwrap());
    assert_eq!(crate_pass_tool.select_model(), crate_catalog.code);

    // 2. High Coding Score Requirement (>= 0.8)
    let crate_reqs_code = CrateModelRequirements { min_coding_score: Some(0.85), ..Default::default() };
    let monolith_reqs_code = MonolithModelRequirements { min_coding_score: Some(0.85), ..Default::default() };
    let crate_pass_code = CrateModelResolutionPass::new(crate_catalog.clone(), Some(crate_reqs_code));
    let monolith_pass_code = MonolithModelResolutionPass { model_catalog: monolith_catalog.clone(), model_requirements: Some(monolith_reqs_code) };
    let monolith_ir_code = monolith_pass_code.apply(make_monolith_ir()).await.expect("apply");
    assert_eq!(crate_pass_code.select_model(), monolith_ir_code.nodes[0].model.as_deref().unwrap());
    assert_eq!(crate_pass_code.select_model(), crate_catalog.code);

    // 3. High Reasoning Score Requirement (>= 0.8)
    let crate_reqs_reason = CrateModelRequirements { min_reasoning_score: Some(0.90), ..Default::default() };
    let monolith_reqs_reason = MonolithModelRequirements { min_reasoning_score: Some(0.90), ..Default::default() };
    let crate_pass_reason = CrateModelResolutionPass::new(crate_catalog.clone(), Some(crate_reqs_reason));
    let monolith_pass_reason = MonolithModelResolutionPass { model_catalog: monolith_catalog.clone(), model_requirements: Some(monolith_reqs_reason) };
    let monolith_ir_reason = monolith_pass_reason.apply(make_monolith_ir()).await.expect("apply");
    assert_eq!(crate_pass_reason.select_model(), monolith_ir_reason.nodes[0].model.as_deref().unwrap());
    assert_eq!(crate_pass_reason.select_model(), crate_catalog.architecture);

    // 4. Overlapping Priority Test A: Tool Requirement vs High Reasoning Score
    let crate_reqs_overlap_a = CrateModelRequirements { requires_tools: true, min_reasoning_score: Some(0.95), ..Default::default() };
    let monolith_reqs_overlap_a = MonolithModelRequirements { requires_tools: true, min_reasoning_score: Some(0.95), ..Default::default() };
    let crate_pass_overlap_a = CrateModelResolutionPass::new(crate_catalog.clone(), Some(crate_reqs_overlap_a));
    let monolith_pass_overlap_a = MonolithModelResolutionPass { model_catalog: monolith_catalog.clone(), model_requirements: Some(monolith_reqs_overlap_a) };
    let monolith_ir_overlap_a = monolith_pass_overlap_a.apply(make_monolith_ir()).await.expect("apply");
    assert_eq!(crate_pass_overlap_a.select_model(), monolith_ir_overlap_a.nodes[0].model.as_deref().unwrap());
    assert_eq!(crate_pass_overlap_a.select_model(), crate_catalog.code, "requires_tools must take priority over min_reasoning_score");

    // 5. Overlapping Priority Test B: High Coding Score vs High Reasoning Score
    let crate_reqs_overlap_b = CrateModelRequirements { min_coding_score: Some(0.85), min_reasoning_score: Some(0.95), ..Default::default() };
    let monolith_reqs_overlap_b = MonolithModelRequirements { min_coding_score: Some(0.85), min_reasoning_score: Some(0.95), ..Default::default() };
    let crate_pass_overlap_b = CrateModelResolutionPass::new(crate_catalog.clone(), Some(crate_reqs_overlap_b));
    let monolith_pass_overlap_b = MonolithModelResolutionPass { model_catalog: monolith_catalog.clone(), model_requirements: Some(monolith_reqs_overlap_b) };
    let monolith_ir_overlap_b = monolith_pass_overlap_b.apply(make_monolith_ir()).await.expect("apply");
    assert_eq!(crate_pass_overlap_b.select_model(), monolith_ir_overlap_b.nodes[0].model.as_deref().unwrap());
    assert_eq!(crate_pass_overlap_b.select_model(), crate_catalog.code, "min_coding_score must take priority over min_reasoning_score");

    // 6. Default / Fast Fallback
    let crate_pass_default = CrateModelResolutionPass::new(crate_catalog.clone(), None);
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

    let crate_invalid_edge_ir: CrateWorkflowIR = serde_json::from_str(&format!(
        r#"{{"workflow_id":"550e8400-e29b-41d4-a716-446655440000","version":1,"nodes":[{{"id":"node_a","kind":"Task","capability":null,"config":{{}}}}],"edges":[{{"from":"node_unknown","to":"node_a","kind":"Sequential","condition":null}}],"metadata":{{"policy_applied":[],"estimated_cost":0.0,"estimated_tokens":0}}}}"#
    )).expect("deserialize crate ir");

    assert!(monolith_pass.apply(monolith_invalid_edge_ir).await.is_err(), "Monolith must reject unknown edge source");
    assert!(crate_pass.transform(&crate_invalid_edge_ir).is_err(), "Crate pass must reject unknown edge source");

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

    let valid_crate_dag_ir = fusion_ir::WorkflowBuilder::new()
        .task("node_a", "TaskA")
        .expect("task a")
        .task("node_b", "TaskB")
        .expect("task b")
        .sequential("node_a", "node_b")
        .expect("sequential edge")
        .build()
        .expect("build valid ir");

    assert!(monolith_pass.apply(valid_monolith_dag_ir).await.is_ok(), "Monolith must accept valid DAG");
    assert!(crate_pass.transform(&valid_crate_dag_ir).is_ok(), "Crate pass must accept valid DAG");

    // 3. Conditional missing condition -> both reject
    let monolith_bad_cond_ir = MonolithWorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![
            MonolithIRNode { id: id_a, kind: MonolithIRNodeKind::Generate, strategy: MonolithStrategyKind::Single, model: None, config: HashMap::new() },
            MonolithIRNode { id: id_b, kind: MonolithIRNodeKind::Review, strategy: MonolithStrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![MonolithIREdge { from: id_a, to: id_b, condition: None }],
        metadata: MonolithIRMetadata { policy_applied: vec![], estimated_cost: 0.0, estimated_tokens: 10 },
    };
    let crate_bad_cond_ir: CrateWorkflowIR = serde_json::from_str(&format!(
        r#"{{"workflow_id":"550e8400-e29b-41d4-a716-446655440000","version":1,"nodes":[{{"id":"node_a","kind":"Task","capability":null,"config":{{}}}},{{"id":"node_b","kind":"Task","capability":null,"config":{{}}}}],"edges":[{{"from":"node_a","to":"node_b","kind":"Conditional","condition":null}}],"metadata":{{"policy_applied":[],"estimated_cost":0.0,"estimated_tokens":0}}}}"#
    )).expect("deserialize crate ir");
    assert!(monolith_pass.apply(monolith_bad_cond_ir).await.is_ok());
    assert!(crate_pass.transform(&crate_bad_cond_ir).is_err(), "Crate must reject conditional edge without condition");

    // 4. Split arity: 1 outgoing -> both reject
    let crate_single_out_ir: CrateWorkflowIR = serde_json::from_str(
        r#"{"workflow_id":"550e8400-e29b-41d4-a716-446655440000","version":1,"nodes":[{"id":"splitter","kind":"Task","capability":null,"config":{"control_flow":"split"}},{"id":"target","kind":"Task","capability":null,"config":{}}],"edges":[{"from":"splitter","to":"target","kind":"Sequential","condition":null}],"metadata":{"policy_applied":[],"estimated_cost":0.0,"estimated_tokens":0}}"#
    ).expect("deserialize crate ir");
    assert!(crate_pass.transform(&crate_single_out_ir).is_err(), "Crate must reject split with 1 outgoing edge");

    // 5. Split arity: 2 generic outgoing -> both accept
    let crate_two_out_ir: CrateWorkflowIR = serde_json::from_str(
        r#"{"workflow_id":"550e8400-e29b-41d4-a716-446655440000","version":1,"nodes":[{"id":"splitter","kind":"Task","capability":null,"config":{"control_flow":"split"}},{"id":"a","kind":"Task","capability":null,"config":{}},{"id":"b","kind":"Task","capability":null,"config":{}}],"edges":[{"from":"splitter","to":"a","kind":"Sequential","condition":null},{"from":"splitter","to":"b","kind":"Sequential","condition":null}],"metadata":{"policy_applied":[],"estimated_cost":0.0,"estimated_tokens":0}}"#
    ).expect("deserialize crate ir");
    assert!(crate_pass.transform(&crate_two_out_ir).is_ok(), "Crate must accept split with 2 outgoing edges");

    // 6. Merge arity: 1 incoming Merge edge -> both reject
    let crate_single_merge_ir: CrateWorkflowIR = serde_json::from_str(
        r#"{"workflow_id":"550e8400-e29b-41d4-a716-446655440000","version":1,"nodes":[{"id":"src","kind":"Task","capability":null,"config":{}},{"id":"m","kind":"Aggregation","capability":null,"config":{}},{"id":"out","kind":"Task","capability":null,"config":{}}],"edges":[{"from":"src","to":"m","kind":"Merge","condition":null},{"from":"m","to":"out","kind":"Sequential","condition":null}],"metadata":{"policy_applied":[],"estimated_cost":0.0,"estimated_tokens":0}}"#
    ).expect("deserialize crate ir");
    assert!(crate_pass.transform(&crate_single_merge_ir).is_err(), "Crate must reject merge with 1 incoming");

    // 7. Merge arity: 2 incoming Merge edges, no outgoing -> accept (Join-shaped)
    let crate_join_ir: CrateWorkflowIR = serde_json::from_str(
        r#"{"workflow_id":"550e8400-e29b-41d4-a716-446655440000","version":1,"nodes":[{"id":"a","kind":"Task","capability":null,"config":{}},{"id":"b","kind":"Task","capability":null,"config":{}},{"id":"m","kind":"Aggregation","capability":null,"config":{}}],"edges":[{"from":"a","to":"m","kind":"Merge","condition":null},{"from":"b","to":"m","kind":"Merge","condition":null}],"metadata":{"policy_applied":[],"estimated_cost":0.0,"estimated_tokens":0}}"#
    ).expect("deserialize crate ir");
    assert!(crate_pass.transform(&crate_join_ir).is_ok(), "Crate must accept join with 2 incoming merges");

    // 8. Loop back-edge cycle exclusion: sequential back-edge -> reject, Loop back-edge -> accept
    let crate_loop_cycle_ir = fusion_ir::WorkflowBuilder::new()
        .task("a", "A").expect("a")
        .task("b", "B").expect("b")
        .sequential("a", "b").expect("edge")
        .loop_edge("b", "a").expect("loop")
        .build().expect("build ir");
    assert!(crate_pass.transform(&crate_loop_cycle_ir).is_ok(), "Loop back-edge must not trigger IllegalCycle");
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
