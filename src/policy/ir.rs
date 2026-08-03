//! Phase 4A2 — `PolicyIR` & Normalization Engine (`src/policy/ir.rs`)
//!
//! Normalized, immutable compiler Intermediate Representation of policies.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::policy::ast::PolicyAST;
use crate::policy::diagnostics::PolicyDiagnostic;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PolicyEffect {
    Deny,     // Precedence 0 (highest)
    Approval, // Precedence 1
    Allow,    // Precedence 2 (lowest)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyCondition {
    pub field: String,
    pub expected: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyAction {
    pub action_type: String,
    pub parameters: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub rule_id: String,
    pub target_pattern: String,
    pub priority: u32,
    pub effect: PolicyEffect,
    pub conditions: Vec<PolicyCondition>,
    pub actions: Vec<PolicyAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyIR {
    pub rules: Vec<PolicyRule>,
}

impl PolicyIR {
    /// Normalizes a high-level `PolicyAST` into a compiler `PolicyIR`.
    ///
    /// Fail-closed: an unrecognized `effect` string is a hard error instead of
    /// silently downgrading the rule to `Allow` (ADR-034 / v0.13.1 charter WP 1.1).
    pub fn from_ast(ast: &PolicyAST) -> Result<Self, PolicyDiagnostic> {
        let mut rules = Vec::new();

        for decl in &ast.declarations {
            let effect = match decl.effect.as_str() {
                "deny" => PolicyEffect::Deny,
                "approval" => PolicyEffect::Approval,
                "allow" => PolicyEffect::Allow,
                other => {
                    return Err(PolicyDiagnostic::error(
                        format!("declaration '{}'", decl.name),
                        Some(decl.name.clone()),
                        format!(
                            "Invalid effect '{}'. Expected one of: deny, approval, allow",
                            other
                        ),
                    ));
                }
            };

            let conditions = decl
                .conditions
                .iter()
                .map(|(k, v)| PolicyCondition {
                    field: k.clone(),
                    expected: v.clone(),
                })
                .collect();

            rules.push(PolicyRule {
                rule_id: decl.name.clone(),
                target_pattern: decl.match_target.clone(),
                priority: decl.priority,
                effect,
                conditions,
                actions: Vec::new(),
            });
        }

        // Sort rules by explicit effect precedence (Deny > Approval > Allow) then by priority
        rules.sort_by(|a, b| {
            a.effect
                .cmp(&b.effect)
                .then_with(|| b.priority.cmp(&a.priority))
        });

        Ok(Self { rules })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::ast::PolicyParser;

    #[test]
    fn test_policy_ir_normalization_and_precedence_sorting() {
        let json_raw = r#"{
            "version": "1.0",
            "declarations": [
                {
                    "name": "allow-rule",
                    "priority": 10,
                    "match_target": "shell.exec",
                    "effect": "allow",
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

        assert_eq!(ir.rules.len(), 2);
        assert_eq!(ir.rules[0].effect, PolicyEffect::Deny); // Deny takes precedence over Allow!
    }

    #[test]
    fn test_from_ast_rejects_unknown_effect() {
        let json_raw = r#"{
            "version": "1.0",
            "declarations": [
                {
                    "name": "typo-rule",
                    "priority": 100,
                    "match_target": "shell.exec",
                    "effect": "denyy",
                    "conditions": {},
                    "annotations": {}
                }
            ]
        }"#;

        let (ast, diagnostics) = PolicyParser::parse_json(json_raw).unwrap();
        assert!(!diagnostics.is_empty(), "parser should flag the invalid effect");

        let result = PolicyIR::from_ast(&ast);
        assert!(result.is_err(), "unknown effect must fail closed, not default to Allow");
    }

    #[test]
    fn test_from_ast_rejects_uppercase_effect() {
        let json_raw = r#"{
            "version": "1.0",
            "declarations": [
                {
                    "name": "case-rule",
                    "priority": 100,
                    "match_target": "shell.exec",
                    "effect": "Deny",
                    "conditions": {},
                    "annotations": {}
                }
            ]
        }"#;

        let (ast, _) = PolicyParser::parse_json(json_raw).unwrap();
        assert!(PolicyIR::from_ast(&ast).is_err(), "case-mismatched effects must fail closed");
    }
}
