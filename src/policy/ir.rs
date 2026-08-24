//! Phase 4A2 — `PolicyIR` & Normalization Engine (`src/policy/ir.rs`)
//!
//! Normalized, immutable compiler Intermediate Representation of policies.

use crate::policy::ast::PolicyAST;
use crate::policy::diagnostics::PolicyDiagnostic;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

    /// Builds a compiler `PolicyIR` from the live runtime registry snapshot.
    ///
    /// Each registry entry stores the full serialized `PolicyDeclaration`.
    /// Fail-closed: an unparseable entry or unknown effect is a hard error,
    /// never silently downgraded (ADR-034 charter WP 1.1).
    pub fn from_policy_snapshot(snap: &fusion_planner::PolicySnapshot) -> Result<Self, String> {
        if snap.policies.is_empty() {
            return Ok(Self { rules: Vec::new() });
        }
        let mut rules = Vec::with_capacity(snap.policies.len());
        let mut errors = Vec::new();
        for decl_snap in &snap.policies {
            let decl: crate::policy::ast::PolicyDeclaration =
                match serde_json::from_str(&decl_snap.rule) {
                    Ok(d) => d,
                    Err(e) => {
                        errors.push(format!(
                            "policy '{}' (id {}): stored rule is not a valid declaration: {e}",
                            decl_snap.name, decl_snap.id
                        ));
                        continue;
                    }
                };
            let effect = match decl.effect.as_str() {
                "deny" => PolicyEffect::Deny,
                "approval" => PolicyEffect::Approval,
                "allow" => PolicyEffect::Allow,
                other => {
                    errors.push(format!(
                        "policy '{}' has invalid effect '{other}'",
                        decl.name
                    ));
                    continue;
                }
            };
            rules.push(PolicyRule {
                rule_id: decl.name.clone(),
                target_pattern: decl.match_target.clone(),
                priority: decl.priority,
                effect,
                conditions: decl
                    .conditions
                    .iter()
                    .map(|(k, v)| PolicyCondition {
                        field: k.clone(),
                        expected: v.clone(),
                    })
                    .collect(),
                actions: Vec::new(),
            });
        }
        if !errors.is_empty() {
            return Err(errors.join("; "));
        }
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

    fn snapshot_with(
        decls: &[crate::policy::ast::PolicyDeclaration],
    ) -> fusion_planner::PolicySnapshot {
        use fusion_planner::PolicyDeclarationSnapshot;
        fusion_planner::PolicySnapshot {
            version: 2,
            policies: decls
                .iter()
                .map(|d| PolicyDeclarationSnapshot {
                    id: d.name.clone(),
                    name: d.name.clone(),
                    rule: serde_json::to_string(d).unwrap(),
                })
                .collect(),
            created_at: 0,
        }
    }

    fn declaration(
        name: &str,
        target: &str,
        effect: &str,
        priority: u32,
    ) -> crate::policy::ast::PolicyDeclaration {
        crate::policy::ast::PolicyDeclaration {
            name: name.into(),
            priority,
            match_target: target.into(),
            effect: effect.into(),
            conditions: Default::default(),
            annotations: Default::default(),
        }
    }

    #[test]
    fn from_policy_snapshot_preserves_effects_and_precedence() {
        let snap = snapshot_with(&[
            declaration("allow-all", "*", "allow", 100),
            declaration("deny-shell", "shell.exec", "deny", 1),
        ]);
        let ir = PolicyIR::from_policy_snapshot(&snap).unwrap();
        assert_eq!(ir.rules.len(), 2);
        assert_eq!(
            ir.rules[0].effect,
            PolicyEffect::Deny,
            "deny must sort first"
        );
        // The bridged IR used by the compiler must also resolve deny as winner.
        let bridged: fusion_compiler::policy::PolicyIR = ir.into();
        let rule = fusion_compiler::policy::PolicyPrecedenceEngine::evaluate_matching_rule(
            &bridged,
            "shell.exec",
        )
        .expect("rule must match target");
        assert_eq!(rule.effect, fusion_compiler::policy::PolicyEffect::Deny);
    }

    #[test]
    fn from_policy_snapshot_fails_closed_on_garbage_rule() {
        let snap = fusion_planner::PolicySnapshot {
            version: 3,
            policies: vec![fusion_planner::PolicyDeclarationSnapshot {
                id: "bad".into(),
                name: "bad".into(),
                rule: "not json at all".into(),
            }],
            created_at: 0,
        };
        let result = PolicyIR::from_policy_snapshot(&snap);
        assert!(result.is_err(), "unparseable stored rules must be rejected");
        assert!(result.unwrap_err().contains("bad"));
    }

    #[test]
    fn from_policy_snapshot_fails_closed_on_unknown_effect() {
        let snap = snapshot_with(&[declaration("typo", "shell.exec", "denyy", 5)]);
        assert!(PolicyIR::from_policy_snapshot(&snap).is_err());
    }

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
        assert!(
            !diagnostics.is_empty(),
            "parser should flag the invalid effect"
        );

        let result = PolicyIR::from_ast(&ast);
        assert!(
            result.is_err(),
            "unknown effect must fail closed, not default to Allow"
        );
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
        assert!(
            PolicyIR::from_ast(&ast).is_err(),
            "case-mismatched effects must fail closed"
        );
    }
}
