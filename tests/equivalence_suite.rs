//! Equivalence Test Suite — Pre-Convergence Monolith vs. Workspace Crates
//!
//! Diffs the outputs of ported passes in `crates/fusion-compiler` directly
//! against the historical monolith implementations preserved in `tests/legacy_reference/`
//! (extracted from git commit `9dcd55a4f7c4d9b46f89e0381afabdc0043d1c66~1`).
//!
//! Purpose: Prove that the compiler pass port to `crates/` preserved 100% behavioral equivalence.

mod legacy_reference;

use legacy_reference::{
    LegacyCompilerPass,
    LegacyConstraintValidationPass,
    LegacyControlFlowValidationPass,
    LegacyModelResolutionPass,
};
use fusion_compiler::{
    CompilerPass as CrateCompilerPass,
    ConstraintValidationPass as CrateConstraintPass,
    ControlFlowValidationPass as CrateControlFlowPass,
    ModelResolutionPass as CrateModelResolutionPass,
};
use fusion_types::{WorkflowIR, IRNode, IRNodeKind, IREdge, StrategyKind, ModelCatalog, NanoUSD};
use uuid::Uuid;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// 1. ConstraintValidationPass Equivalence
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_constraint_validation_pass_equivalence() {
    let legacy_pass = LegacyConstraintValidationPass;
    let crate_pass = CrateConstraintPass;

    // Case 1: Empty IR -> both legacy and crate must reject
    let empty_ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![],
        edges: vec![],
        metadata: fusion_types::IRMetadata {
            policy_version: 0,
            policy_applied: vec![],
            estimated_cost: NanoUSD::ZERO,
            estimated_tokens: 0,
        },
    };

    let legacy_res = legacy_pass.apply(empty_ir.clone()).await;
    let crate_res = crate_pass.apply(empty_ir).await;

    assert!(legacy_res.is_err(), "Legacy pass must reject empty IR");
    assert!(crate_res.is_err(), "Crate pass must reject empty IR");

    // Case 2: Valid IR -> both legacy and crate must accept and return identical IR
    let valid_ir = WorkflowIR {
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
            estimated_cost: NanoUSD::ZERO,
            estimated_tokens: 10,
        },
    };

    let legacy_valid_res = legacy_pass.apply(valid_ir.clone()).await.expect("legacy valid");
    let crate_valid_res = crate_pass.apply(valid_ir).await.expect("crate valid");

    assert_eq!(legacy_valid_res.nodes.len(), crate_valid_res.nodes.len());
    assert_eq!(legacy_valid_res.nodes[0].id, crate_valid_res.nodes[0].id);
    assert_eq!(legacy_valid_res.nodes[0].kind, crate_valid_res.nodes[0].kind);
}

// ---------------------------------------------------------------------------
// 2. ModelResolutionPass Equivalence
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_model_resolution_pass_equivalence() {
    let catalog = ModelCatalog {
        code: "gpt-4o".into(),
        debug: "gpt-4o".into(),
        architecture: "claude-3-5-sonnet".into(),
        general: "gpt-4o-mini".into(),
        creative: "claude-3-5-sonnet".into(),
        analysis: "gpt-4o".into(),
        fast: "gpt-4o-mini".into(),
        cheap: "gpt-4o-mini".into(),
    };

    let legacy_pass = LegacyModelResolutionPass::new(catalog.clone());
    let crate_pass = CrateModelResolutionPass::new(catalog);

    // Parity check on select_model()
    assert_eq!(legacy_pass.select_model(), crate_pass.select_model());

    let make_unresolved_ir = || WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![
            IRNode { id: Uuid::new_v4(), kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: Uuid::new_v4(), kind: IRNodeKind::Conditional, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![],
        metadata: fusion_types::IRMetadata {
            policy_version: 0,
            policy_applied: vec![],
            estimated_cost: NanoUSD::ZERO,
            estimated_tokens: 10,
        },
    };

    let legacy_out = legacy_pass.apply(make_unresolved_ir()).await.expect("legacy apply");
    let crate_out = crate_pass.apply(make_unresolved_ir()).await.expect("crate apply");

    assert_eq!(legacy_out.nodes[0].model, crate_out.nodes[0].model);
    assert_eq!(legacy_out.nodes[1].model, None, "Conditional nodes must not have models assigned");
    assert_eq!(crate_out.nodes[1].model, None, "Conditional nodes must not have models assigned");
}

// ---------------------------------------------------------------------------
// 3. ControlFlowValidationPass Equivalence
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_control_flow_validation_pass_equivalence() {
    let legacy_pass = LegacyControlFlowValidationPass;
    let crate_pass = CrateControlFlowPass;

    // 1. Dangling edge -> both must reject
    let id_a = Uuid::new_v4();
    let id_bad = Uuid::new_v4();
    let dangling_ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![IRNode {
            id: id_a,
            kind: IRNodeKind::Generate,
            strategy: StrategyKind::Single,
            model: None,
            config: HashMap::new(),
        }],
        edges: vec![IREdge {
            from: id_bad,
            to: id_a,
            condition: None,
        }],
        metadata: fusion_types::IRMetadata {
            policy_version: 0,
            policy_applied: vec![],
            estimated_cost: NanoUSD::ZERO,
            estimated_tokens: 0,
        },
    };

    assert!(legacy_pass.apply(dangling_ir.clone()).await.is_err(), "Legacy must reject dangling edge");
    assert!(crate_pass.apply(dangling_ir).await.is_err(), "Crate must reject dangling edge");

    // 2. Valid multi-node DAG -> both must accept
    let id_b = Uuid::new_v4();
    let valid_dag = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![
            IRNode { id: id_a, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: id_b, kind: IRNodeKind::Review, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![IREdge {
            from: id_a,
            to: id_b,
            condition: None,
        }],
        metadata: fusion_types::IRMetadata {
            policy_version: 0,
            policy_applied: vec![],
            estimated_cost: NanoUSD::ZERO,
            estimated_tokens: 10,
        },
    };

    assert!(legacy_pass.apply(valid_dag.clone()).await.is_ok(), "Legacy must accept valid DAG");
    assert!(crate_pass.apply(valid_dag).await.is_ok(), "Crate must accept valid DAG");

    // 3. Illegal Cycle Detection -> both must detect and reject
    let cyclic_dag = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![
            IRNode { id: id_a, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: id_b, kind: IRNodeKind::Review, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![
            IREdge { from: id_a, to: id_b, condition: None },
            IREdge { from: id_b, to: id_a, condition: None },
        ],
        metadata: fusion_types::IRMetadata {
            policy_version: 0,
            policy_applied: vec![],
            estimated_cost: NanoUSD::ZERO,
            estimated_tokens: 10,
        },
    };

    assert!(legacy_pass.apply(cyclic_dag.clone()).await.is_err(), "Legacy must reject illegal cycle");
    assert!(crate_pass.apply(cyclic_dag).await.is_err(), "Crate must reject illegal cycle");
}
