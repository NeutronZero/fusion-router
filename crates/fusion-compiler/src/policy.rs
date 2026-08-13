//! Minimal policy types extracted from `src/policy/`.
//!
//! Contains only the surface needed by `PolicyCompilerPass`:
//! `PolicyIR`, `PolicyRule`, `PolicyEffect`, rule matching, and precedence logic.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyIR {
    pub rules: Vec<PolicyRule>,
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PolicyEffect {
    Deny,
    Approval,
    Allow,
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

// ---------------------------------------------------------------------------
// Precedence engine
// ---------------------------------------------------------------------------

pub struct PolicyPrecedenceEngine;

impl PolicyPrecedenceEngine {
    /// Returns the single highest-precedence rule matching `target_symbol`.
    ///
    /// Matching: exact string equality OR wildcard `"*"`.
    /// Precedence: `Deny` > `Approval` > `Allow` (via `Reverse` on Ord), then
    /// higher `priority` wins within the same effect.
    pub fn evaluate_matching_rule<'a>(
        ir: &'a PolicyIR,
        target_symbol: &str,
    ) -> Option<&'a PolicyRule> {
        ir.rules
            .iter()
            .filter(|r| r.target_pattern == target_symbol || r.target_pattern == "*")
            .max_by_key(|r| (std::cmp::Reverse(&r.effect), r.priority))
    }
}

// ---------------------------------------------------------------------------
// Trace (for diagnostics; minimal)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum PolicyMatchEvent {
    RuleMatched {
        rule_id: String,
        symbol: String,
        effect: PolicyEffect,
    },
    NodeInserted {
        gate_id: uuid::Uuid,
        target_node_id: uuid::Uuid,
    },
}

#[derive(Debug, Clone, Default)]
pub struct PolicyTrace {
    pub events: Vec<PolicyMatchEvent>,
}

impl PolicyTrace {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn record(&mut self, event: PolicyMatchEvent) {
        self.events.push(event);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn deny_rule(id: &str, target: &str, priority: u32) -> PolicyRule {
        PolicyRule {
            rule_id: id.into(),
            target_pattern: target.into(),
            priority,
            effect: PolicyEffect::Deny,
            conditions: vec![],
            actions: vec![],
        }
    }

    fn approval_rule(id: &str, target: &str, priority: u32) -> PolicyRule {
        PolicyRule {
            rule_id: id.into(),
            target_pattern: target.into(),
            priority,
            effect: PolicyEffect::Approval,
            conditions: vec![],
            actions: vec![],
        }
    }

    fn allow_rule(id: &str, target: &str, priority: u32) -> PolicyRule {
        PolicyRule {
            rule_id: id.into(),
            target_pattern: target.into(),
            priority,
            effect: PolicyEffect::Allow,
            conditions: vec![],
            actions: vec![],
        }
    }

    #[test]
    fn test_deny_beats_approval() {
        let ir = PolicyIR {
            rules: vec![approval_rule("a1", "shell.exec", 100), deny_rule("d1", "shell.exec", 1)],
        };
        let rule = PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "shell.exec").unwrap();
        assert_eq!(rule.effect, PolicyEffect::Deny);
        assert_eq!(rule.rule_id, "d1");
    }

    #[test]
    fn test_precedence_is_order_independent() {
        let ir = PolicyIR {
            rules: vec![allow_rule("a1", "*", 100), deny_rule("d1", "shell.exec", 1)],
        };
        let rule = PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "shell.exec").unwrap();
        assert_eq!(rule.effect, PolicyEffect::Deny);
    }

    #[test]
    fn test_higher_priority_wins_within_same_effect() {
        let ir = PolicyIR {
            rules: vec![deny_rule("d1", "x", 10), deny_rule("d2", "x", 100)],
        };
        let rule = PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "x").unwrap();
        assert_eq!(rule.rule_id, "d2");
    }

    #[test]
    fn test_wildcard_matches_all() {
        let ir = PolicyIR {
            rules: vec![approval_rule("a1", "*", 50)],
        };
        let rule = PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "anything").unwrap();
        assert_eq!(rule.rule_id, "a1");
    }

    #[test]
    fn test_no_match_returns_none() {
        let ir = PolicyIR {
            rules: vec![deny_rule("d1", "shell.exec", 1)],
        };
        assert!(PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "web.fetch").is_none());
    }

    #[test]
    fn test_effect_ord_deny_highest() {
        assert!(PolicyEffect::Deny < PolicyEffect::Approval);
        assert!(PolicyEffect::Approval < PolicyEffect::Allow);
    }
}
