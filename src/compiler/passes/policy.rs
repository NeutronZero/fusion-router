//! Phase 4B1 — `PolicyCompilerPass` (`src/compiler/passes/policy.rs`)
//!
//! Additive compiler pass lowering declarative `PolicyIR` rules into `Gate` nodes in `WorkflowIR`.

use async_trait::async_trait;
use uuid::Uuid;
use crate::compiler::passes::CompilerPass;
use crate::policy::ir::{PolicyEffect, PolicyIR};
use crate::policy::precedence::PolicyPrecedenceEngine;
use crate::policy::trace::{PolicyMatchEvent, PolicyTrace};
use crate::types::{CompilerError, IREdge, IRNode, IRNodeKind, StrategyKind, WorkflowIR};

pub struct PolicyCompilerPass {
    policy_ir: PolicyIR,
}

impl PolicyCompilerPass {
    pub fn new(policy_ir: PolicyIR) -> Self {
        Self { policy_ir }
    }
}

#[async_trait]
impl CompilerPass for PolicyCompilerPass {
    fn name(&self) -> &str {
        "PolicyCompilerPass"
    }

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        let mut new_nodes = ir.nodes.clone();
        let mut new_edges = ir.edges.clone();
        let mut trace = PolicyTrace::new();
        let mut inserted_gate_nodes = Vec::new();

        for node in &ir.nodes {
            let symbol_key = node
                .config
                .get("capability")
                .and_then(|v| v.as_str())
                .or(node.model.as_deref())
                .unwrap_or("general");

            if let Some(rule) = PolicyPrecedenceEngine::evaluate_matching_rule(&self.policy_ir, symbol_key) {
                trace.record(PolicyMatchEvent::RuleMatched {
                    node_id: node.id,
                    rule_id: rule.rule_id.clone(),
                    target_pattern: rule.target_pattern.clone(),
                    effect: rule.effect.clone(),
                });

                if rule.effect == PolicyEffect::Deny {
                    // A matched Deny rule is a hard compile error (ADR-034 / Law 2):
                    // no ExecutionGraph may be produced for a workflow that violates
                    // a deny policy. Fail before any Approval handling.
                    return Err(CompilerError::ValidationError {
                        pass: "PolicyCompilerPass".to_string(),
                        node_id: Some(node.id),
                        message: format!(
                            "Policy rule '{}' denies target '{}' (effect: deny); node {} cannot be compiled",
                            rule.rule_id, rule.target_pattern, node.id
                        ),
                    });
                }

                if rule.effect == PolicyEffect::Approval {
                    // Idempotence check: check if an approval gate edge pointing to node.id already exists
                    let already_guarded = new_edges.iter().any(|edge| {
                        edge.to == node.id
                            && new_nodes.iter().any(|n| n.id == edge.from && n.kind == IRNodeKind::Gate)
                    });

                    if !already_guarded {
                        let gate_id = Uuid::new_v4();
                        let gate_node = IRNode {
                            id: gate_id,
                            kind: IRNodeKind::Gate,
                            strategy: StrategyKind::Single,
                            model: Some("policy.approval_gate".into()),
                            config: std::collections::HashMap::new(),
                        };

                        inserted_gate_nodes.push(gate_node);
                        new_edges.push(IREdge {
                            from: gate_id,
                            to: node.id,
                            condition: None,
                        });

                        trace.record(PolicyMatchEvent::NodeInserted {
                            inserted_node_id: gate_id,
                            node_kind: "Gate".into(),
                            target_node_id: node.id,
                        });
                    }
                }
            }
        }

        new_nodes.extend(inserted_gate_nodes);

        Ok(WorkflowIR {
            plan_id: ir.plan_id,
            nodes: new_nodes,
            edges: new_edges,
            metadata: ir.metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::ast::PolicyParser;

    #[tokio::test]
    async fn test_policy_compiler_pass_injects_gate_node() {
        let json_raw = r#"{
            "version": "1.0",
            "declarations": [
                {
                    "name": "require-approval",
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
        let pass = PolicyCompilerPass::new(ir);

        let target_node_id = Uuid::new_v4();
        let mut config = std::collections::HashMap::new();
        config.insert("capability".into(), serde_json::json!("shell.exec"));

        let input_ir = WorkflowIR {
            plan_id: Uuid::new_v4(),
            nodes: vec![IRNode {
                id: target_node_id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some("gpt-4o".into()),
                config,
            }],
            edges: vec![],
            metadata: crate::types::IRMetadata {
                policy_applied: vec![],
                estimated_cost: crate::types::NanoUSD::ZERO,
                estimated_tokens: 100,
            },
        };

        let output_ir = pass.apply(input_ir).await.unwrap();

        assert_eq!(output_ir.nodes.len(), 2); // 1 original + 1 injected GateNode!
        assert_eq!(output_ir.edges.len(), 1);
    }

    #[tokio::test]
    async fn test_deny_rule_blocks_compilation() {
        let json_raw = r#"{
            "version": "1.0",
            "declarations": [
                {
                    "name": "deny-shell",
                    "priority": 100,
                    "match_target": "shell.exec",
                    "effect": "deny",
                    "conditions": {},
                    "annotations": {}
                }
            ]
        }"#;

        let (ast, _) = PolicyParser::parse_json(json_raw).unwrap();
        let ir = PolicyIR::from_ast(&ast).unwrap();
        let pass = PolicyCompilerPass::new(ir);

        let target_node_id = Uuid::new_v4();
        let mut config = std::collections::HashMap::new();
        config.insert("capability".into(), serde_json::json!("shell.exec"));

        let input_ir = WorkflowIR {
            plan_id: Uuid::new_v4(),
            nodes: vec![IRNode {
                id: target_node_id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some("gpt-4o".into()),
                config,
            }],
            edges: vec![],
            metadata: crate::types::IRMetadata {
                policy_applied: vec![],
                estimated_cost: crate::types::NanoUSD::ZERO,
                estimated_tokens: 100,
            },
        };

        let result = pass.apply(input_ir).await;
        assert!(result.is_err(), "a matched Deny rule must fail compilation");
        let err = result.unwrap_err();
        match err {
            crate::types::CompilerError::ValidationError { pass, node_id, message } => {
                assert_eq!(pass, "PolicyCompilerPass");
                assert_eq!(node_id, Some(target_node_id));
                assert!(message.contains("deny-shell"), "message must identify the deny rule: {message}");
            }
            other => panic!("expected ValidationError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_deny_outranks_approval_on_same_target() {
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
                },
                {
                    "name": "deny-rule",
                    "priority": 1,
                    "match_target": "shell.exec",
                    "effect": "deny",
                    "conditions": {},
                    "annotations": {}
                }
            ]
        }"#;

        let (ast, _) = PolicyParser::parse_json(json_raw).unwrap();
        let ir = PolicyIR::from_ast(&ast).unwrap();
        let pass = PolicyCompilerPass::new(ir);

        let mut config = std::collections::HashMap::new();
        config.insert("capability".into(), serde_json::json!("shell.exec"));

        let input_ir = WorkflowIR {
            plan_id: Uuid::new_v4(),
            nodes: vec![IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some("gpt-4o".into()),
                config,
            }],
            edges: vec![],
            metadata: crate::types::IRMetadata {
                policy_applied: vec![],
                estimated_cost: crate::types::NanoUSD::ZERO,
                estimated_tokens: 100,
            },
        };

        let result = pass.apply(input_ir).await;
        assert!(result.is_err(), "Deny must win over Approval regardless of priority");
    }

    #[tokio::test]
    async fn test_unrelated_target_is_not_denied() {
        let json_raw = r#"{
            "version": "1.0",
            "declarations": [
                {
                    "name": "deny-shell",
                    "priority": 100,
                    "match_target": "shell.exec",
                    "effect": "deny",
                    "conditions": {},
                    "annotations": {}
                }
            ]
        }"#;

        let (ast, _) = PolicyParser::parse_json(json_raw).unwrap();
        let ir = PolicyIR::from_ast(&ast).unwrap();
        let pass = PolicyCompilerPass::new(ir);

        let mut config = std::collections::HashMap::new();
        config.insert("capability".into(), serde_json::json!("web.fetch"));

        let input_ir = WorkflowIR {
            plan_id: Uuid::new_v4(),
            nodes: vec![IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some("gpt-4o".into()),
                config,
            }],
            edges: vec![],
            metadata: crate::types::IRMetadata {
                policy_applied: vec![],
                estimated_cost: crate::types::NanoUSD::ZERO,
                estimated_tokens: 100,
            },
        };

        let output_ir = pass.apply(input_ir).await.expect("unrelated node must pass");
        assert_eq!(output_ir.nodes.len(), 1);
    }
}
