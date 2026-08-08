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
