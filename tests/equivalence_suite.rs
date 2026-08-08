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
}
