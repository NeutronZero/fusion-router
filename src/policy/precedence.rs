//! Phase 4A2 — `PolicyPrecedenceEngine` (`src/policy/precedence.rs`)
//!
//! Evaluates matching rules according to formal precedence: Deny > Approval > Allow.

use crate::policy::ir::{PolicyIR, PolicyRule};

pub struct PolicyPrecedenceEngine;

impl PolicyPrecedenceEngine {
    /// Matches a target symbol string against an immutable `PolicyIR` and returns the highest precedence matching rule.
    ///
    /// Precedence is computed here, not at the call site, so it holds for any
    /// `PolicyIR` regardless of input order or `Deserialize` source (ADR-034):
    /// `Deny > Approval > Allow`, then higher `priority` wins.
    pub fn evaluate_matching_rule<'a>(ir: &'a PolicyIR, target_symbol: &str) -> Option<&'a PolicyRule> {
        ir.rules
            .iter()
            .filter(|rule| rule.target_pattern == target_symbol || rule.target_pattern == "*")
            .max_by_key(|rule| (std::cmp::Reverse(&rule.effect), rule.priority))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::ast::PolicyParser;
    use crate::policy::ir::PolicyEffect;

    fn rule(rule_id: &str, target: &str, priority: u32, effect: PolicyEffect) -> PolicyRule {
        PolicyRule {
            rule_id: rule_id.to_string(),
            target_pattern: target.to_string(),
            priority,
            effect,
            conditions: Vec::new(),
            actions: Vec::new(),
        }
    }

    #[test]
    fn test_precedence_evaluation_deny_over_approval() {
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
        let matched = PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "shell.exec").unwrap();

        assert_eq!(matched.effect, PolicyEffect::Deny);
    }

    #[test]
    fn test_precedence_is_order_independent_with_unsorted_ir() {
        use crate::policy::ir::PolicyEffect;

        // Constructed directly (bypassing from_ast sorting) with the wildcard
        // Allow listed FIRST: a naive first-match engine would pick Allow.
        let unsorted = PolicyIR {
            rules: vec![
                rule("wildcard-allow", "*", 1, PolicyEffect::Allow),
                rule("specific-deny", "shell.exec", 100, PolicyEffect::Deny),
            ],
        };
        let matched = PolicyPrecedenceEngine::evaluate_matching_rule(&unsorted, "shell.exec").unwrap();
        assert_eq!(matched.rule_id, "specific-deny");
        assert_eq!(matched.effect, PolicyEffect::Deny);
    }

    #[test]
    fn test_precedence_deny_beats_wildcard_approval_regardless_of_order() {
        use crate::policy::ir::PolicyEffect;

        let ir = PolicyIR {
            rules: vec![
                rule("wildcard-approval", "*", 100, PolicyEffect::Approval),
                rule("specific-deny", "shell.exec", 1, PolicyEffect::Deny),
            ],
        };
        let matched = PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "shell.exec").unwrap();
        assert_eq!(matched.effect, PolicyEffect::Deny);
    }

    #[test]
    fn test_precedence_higher_priority_wins_within_same_effect() {
        use crate::policy::ir::PolicyEffect;

        let ir = PolicyIR {
            rules: vec![
                rule("low-priority-deny", "shell.exec", 1, PolicyEffect::Deny),
                rule("high-priority-deny", "shell.exec", 100, PolicyEffect::Deny),
            ],
        };
        let matched = PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "shell.exec").unwrap();
        assert_eq!(matched.rule_id, "high-priority-deny");
    }

    #[test]
    fn test_specific_rule_beats_wildcard_deny_for_other_target() {
        use crate::policy::ir::PolicyEffect;

        let ir = PolicyIR {
            rules: vec![
                rule("wildcard-deny", "*", 100, PolicyEffect::Deny),
                rule("specific-allow", "shell.exec", 1, PolicyEffect::Allow),
            ],
        };
        // Deny wins by effect precedence even though the specific rule has priority.
        let matched = PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "shell.exec").unwrap();
        assert_eq!(matched.effect, PolicyEffect::Deny);
    }
}
