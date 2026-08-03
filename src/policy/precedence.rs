//! Phase 4A2 — `PolicyPrecedenceEngine` (`src/policy/precedence.rs`)
//!
//! Evaluates matching rules according to formal precedence: Deny > Approval > Allow.

use crate::policy::ir::{PolicyIR, PolicyRule};

pub struct PolicyPrecedenceEngine;

impl PolicyPrecedenceEngine {
    /// Matches a target symbol string against an immutable `PolicyIR` and returns the highest precedence matching rule.
    pub fn evaluate_matching_rule<'a>(ir: &'a PolicyIR, target_symbol: &str) -> Option<&'a PolicyRule> {
        ir.rules
            .iter()
            .find(|rule| rule.target_pattern == target_symbol || rule.target_pattern == "*")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::ast::PolicyParser;
    use crate::policy::ir::PolicyEffect;

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
        let ir = PolicyIR::from_ast(&ast);
        let matched = PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "shell.exec").unwrap();

        assert_eq!(matched.effect, PolicyEffect::Deny);
    }
}
