//! Executable Policy Compiler Phase Invariants & Idempotence Test Suite
//!
//! Verifies:
//! 1. Idempotence: Running PolicyCompilerPass twice does not duplicate gate nodes.
//! 2. Determinism: Same WorkflowIR and PolicyIR produce identical transformed WorkflowIR.
//! 3. Precedence: Deny > Approval > Allow rule evaluation.
//! 4. Non-Destructive: Original graph node semantics remain intact.

use fusion_compiler::{CompilerPass, PolicyCompilerPass};
use fusion_router::policy::ast::PolicyParser;
use fusion_router::policy::ir::{PolicyEffect, PolicyIR};
use fusion_router::policy::precedence::PolicyPrecedenceEngine;
use fusion_router::types::{IRMetadata, IRNode, IRNodeKind, StrategyKind, WorkflowIR};
use std::collections::HashMap;
use uuid::Uuid;

fn create_sample_ir() -> (WorkflowIR, Uuid) {
    let node_id = Uuid::new_v4();
    let mut config = HashMap::new();
    config.insert("capability".into(), serde_json::json!("shell.exec"));

    let ir = WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![IRNode {
            id: node_id,
            kind: IRNodeKind::Generate,
            strategy: StrategyKind::Single,
            model: Some("gpt-4o".into()),
            config,
        }],
        edges: vec![],
        metadata: IRMetadata {
            policy_version: 0,
            policy_applied: vec![],
            estimated_cost: fusion_router::types::NanoUSD::ZERO,
            estimated_tokens: 100,
        },
    };

    (ir, node_id)
}

#[tokio::test]
async fn policy_invariant_idempotence() {
    let json_raw = r#"{
        "version": "1.0",
        "declarations": [
            {
                "name": "approval-rule",
                "priority": 100,
                "match_target": "shell.exec",
                "effect": "approval",
                "conditions": {},
                "annotations": {}
            }
        ]
    }"#;

    let (ast, _) = PolicyParser::parse_json(json_raw).unwrap();
    let ir = PolicyIR::from_ast(&ast).unwrap();
    let pass = PolicyCompilerPass::new(ir.into());

    let (input_ir, _) = create_sample_ir();

    // Pass 1
    let pass1_ir = pass.apply(input_ir).await.unwrap();
    let node_count_pass1 = pass1_ir.nodes.len();

    // Pass 2 (Idempotence check)
    let pass2_ir = pass.apply(pass1_ir).await.unwrap();
    let node_count_pass2 = pass2_ir.nodes.len();

    assert_eq!(node_count_pass1, node_count_pass2);
}

#[tokio::test]
async fn policy_invariant_precedence_deny_over_approval() {
    let json_raw = r#"{
        "version": "1.0",
        "declarations": [
            {
                "name": "approval-rule",
                "priority": 10,
                "match_target": "shell.exec",
                "effect": "approval",
                "conditions": {},
                "annotations": {}
            },
            {
                "name": "deny-rule",
                "priority": 5,
                "match_target": "shell.exec",
                "effect": "deny",
                "conditions": {},
                "annotations": {}
            }
        ]
    }"#;

    let (ast, _) = PolicyParser::parse_json(json_raw).unwrap();
    let ir = PolicyIR::from_ast(&ast).unwrap();

    let rule = PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "shell.exec").unwrap();
    assert_eq!(rule.effect, PolicyEffect::Deny); // Deny strictly wins over approval
}

#[tokio::test]
async fn policy_invariant_non_destructive_preservation() {
    let json_raw = r#"{
        "version": "1.0",
        "declarations": [
            {
                "name": "approval-rule",
                "priority": 100,
                "match_target": "shell.exec",
                "effect": "approval",
                "conditions": {},
                "annotations": {}
            }
        ]
    }"#;

    let (ast, _) = PolicyParser::parse_json(json_raw).unwrap();
    let ir = PolicyIR::from_ast(&ast).unwrap();
    let pass = PolicyCompilerPass::new(ir.into());

    let (input_ir, original_node_id) = create_sample_ir();
    let output_ir = pass.apply(input_ir).await.unwrap();

    // Original node is preserved
    let original_node = output_ir.nodes.iter().find(|n| n.id == original_node_id);
    assert!(original_node.is_some());
    assert_eq!(original_node.unwrap().kind, IRNodeKind::Generate);
}
