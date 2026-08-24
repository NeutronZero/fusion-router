//! Phase 6.2.3 — explicit policy type bridge: `src/policy` → `fusion_compiler`.
//!
//! The crates `PolicyIR` is the canonical compiler input (used by the crates
//! `PolicyCompilerPass`). The monolith's parser produces `src/policy::ir::PolicyIR`;
//! this `From` impl is the ONLY place the two policy IRs meet. Semantics are
//! identical (same rule/effect/condition shapes); see `crates/fusion-compiler/src/policy.rs`.

use fusion_compiler::policy as fcp;

impl From<crate::policy::ir::PolicyIR> for fcp::PolicyIR {
    fn from(ir: crate::policy::ir::PolicyIR) -> Self {
        fcp::PolicyIR {
            rules: ir
                .rules
                .into_iter()
                .map(|rule| fcp::PolicyRule {
                    rule_id: rule.rule_id,
                    target_pattern: rule.target_pattern,
                    priority: rule.priority,
                    effect: match rule.effect {
                        crate::policy::ir::PolicyEffect::Deny => fcp::PolicyEffect::Deny,
                        crate::policy::ir::PolicyEffect::Approval => fcp::PolicyEffect::Approval,
                        crate::policy::ir::PolicyEffect::Allow => fcp::PolicyEffect::Allow,
                    },
                    conditions: rule
                        .conditions
                        .into_iter()
                        .map(|c| fcp::PolicyCondition {
                            field: c.field,
                            expected: c.expected,
                        })
                        .collect(),
                    actions: rule
                        .actions
                        .into_iter()
                        .map(|a| fcp::PolicyAction {
                            action_type: a.action_type,
                            parameters: a.parameters,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deny_rule(id: &str, target: &str) -> crate::policy::ir::PolicyRule {
        crate::policy::ir::PolicyRule {
            rule_id: id.into(),
            target_pattern: target.into(),
            priority: 10,
            effect: crate::policy::ir::PolicyEffect::Deny,
            conditions: vec![],
            actions: vec![],
        }
    }

    #[test]
    fn converts_ir_shapes_identically() {
        let src_ir = crate::policy::ir::PolicyIR {
            rules: vec![
                deny_rule("deny-shell", "shell.exec"),
                crate::policy::ir::PolicyRule {
                    rule_id: "allow-approve".into(),
                    target_pattern: "*".into(),
                    priority: 5,
                    effect: crate::policy::ir::PolicyEffect::Approval,
                    conditions: vec![crate::policy::ir::PolicyCondition {
                        field: "env".into(),
                        expected: serde_json::json!("prod"),
                    }],
                    actions: vec![crate::policy::ir::PolicyAction {
                        action_type: "notify".into(),
                        parameters: std::collections::HashMap::from([(
                            "channel".into(),
                            serde_json::json!("ops"),
                        )]),
                    }],
                },
            ],
        };

        let converted: fcp::PolicyIR = src_ir.into();
        assert_eq!(converted.rules.len(), 2);
        assert_eq!(converted.rules[0].effect, fcp::PolicyEffect::Deny);
        assert_eq!(converted.rules[0].target_pattern, "shell.exec");
        assert_eq!(converted.rules[1].conditions[0].field, "env");
        assert_eq!(converted.rules[1].actions[0].action_type, "notify");
    }

    #[test]
    fn converted_ir_keeps_precedence_semantics() {
        let src_ir = crate::policy::ir::PolicyIR {
            rules: vec![
                deny_rule("d-low", "shell.exec"),
                crate::policy::ir::PolicyRule {
                    rule_id: "a-high".into(),
                    target_pattern: "shell.exec".into(),
                    priority: 100,
                    effect: crate::policy::ir::PolicyEffect::Allow,
                    conditions: vec![],
                    actions: vec![],
                },
            ],
        };
        let converted: fcp::PolicyIR = src_ir.into();
        let matched = fcp::PolicyPrecedenceEngine::evaluate_matching_rule(&converted, "shell.exec")
            .expect("rule must match");
        assert_eq!(
            matched.effect,
            fcp::PolicyEffect::Deny,
            "deny must win over higher-priority allow"
        );
    }
}
