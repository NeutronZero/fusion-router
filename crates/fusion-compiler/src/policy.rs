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
    /// Matching:
    /// - `target_pattern` supports exact string equality, a lone `"*"`
    ///   wildcard, and embedded `"*"` wildcards (e.g. `"openai/*"`,
    ///   `"*.internal"`) — only the whole-pattern star is special, there is
    ///   no `?` or character-class support.
    /// - A rule with `conditions` matches only when EVERY condition holds
    ///   against `facts` (see [`evaluate_condition`]). An empty condition
    ///   list always matches. Unknown condition fields make the rule
    ///   non-matching (fail toward allow, but logged by the caller).
    /// - Precedence: `Deny` > `Approval` > `Allow` (via `Reverse` on Ord),
    ///   then higher `priority` wins within the same effect.
    pub fn evaluate_matching_rule<'a>(
        ir: &'a PolicyIR,
        target_symbol: &str,
        facts: &PolicyFacts<'_>,
    ) -> Option<&'a PolicyRule> {
        ir.rules
            .iter()
            .filter(|r| Self::pattern_matches(&r.target_pattern, target_symbol))
            .filter(|r| {
                r.conditions
                    .iter()
                    .all(|c| Self::evaluate_condition(c, facts))
            })
            .max_by_key(|r| (std::cmp::Reverse(&r.effect), r.priority))
    }

    /// Whole-pattern `*` wildcard match: `*` alone matches everything; a
    /// pattern of `k` segments split on `*` must align with the candidate's
    /// prefix/suffix/infix structure.
    fn pattern_matches(pattern: &str, candidate: &str) -> bool {
        match pattern.split_once('*') {
            None => pattern == candidate,
            Some((prefix, suffix)) => {
                if !candidate.starts_with(prefix) {
                    return false;
                }
                let rest = &candidate[prefix.len()..];
                if suffix.is_empty() {
                    true
                } else if let Some(middle) = suffix.strip_suffix('*') {
                    // Pattern has 3+ segments (e.g. "a*b*c"): the remainder
                    // must contain each further segment in order.
                    let mut cursor = rest;
                    for seg in middle.split('*') {
                        match cursor.find(seg) {
                            Some(pos) => cursor = &cursor[pos + seg.len()..],
                            None => return false,
                        }
                    }
                    true
                } else {
                    rest.ends_with(suffix)
                }
            }
        }
    }

    /// Evaluates one rule condition against node facts. Supported fields:
    /// `kind`, `model`, `strategy`, `capability`. Equality against the
    /// JSON-encoded expected value. Unknown fields do NOT match (the rule
    /// is skipped rather than applied blindly).
    fn evaluate_condition(condition: &PolicyCondition, facts: &PolicyFacts<'_>) -> bool {
        let actual = match condition.field.as_str() {
            "kind" => facts.node_kind.map(str::to_string),
            "model" => Some(facts.model.to_string()),
            "strategy" => Some(facts.strategy.to_string()),
            "capability" => facts.capability.map(str::to_string),
            _ => None,
        };
        match actual {
            Some(value) => {
                let expected = condition.expected.as_str().map(str::to_string).unwrap_or_else(|| condition.expected.to_string());
                value == expected
            }
            None => false,
        }
    }
}

/// Node-level facts a policy condition can reference.
#[derive(Debug, Clone, Copy, Default)]
pub struct PolicyFacts<'a> {
    pub model: &'a str,
    pub strategy: &'a str,
    pub node_kind: Option<&'a str>,
    pub capability: Option<&'a str>,
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

    fn facts() -> PolicyFacts<'static> {
        PolicyFacts {
            model: "test-model",
            strategy: "Single",
            node_kind: Some("Task"),
            capability: None,
        }
    }

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
            rules: vec![
                approval_rule("a1", "shell.exec", 100),
                deny_rule("d1", "shell.exec", 1),
            ],
        };
        let rule = PolicyPrecedenceEngine::evaluate_matching_rule(
                &ir,
                "shell.exec",
                &facts(),
            ).unwrap();
        assert_eq!(rule.effect, PolicyEffect::Deny);
        assert_eq!(rule.rule_id, "d1");
    }

    #[test]
    fn test_precedence_is_order_independent() {
        let ir = PolicyIR {
            rules: vec![allow_rule("a1", "*", 100), deny_rule("d1", "shell.exec", 1)],
        };
        let rule = PolicyPrecedenceEngine::evaluate_matching_rule(
                &ir,
                "shell.exec",
                &facts(),
            ).unwrap();
        assert_eq!(rule.effect, PolicyEffect::Deny);
    }

    #[test]
    fn test_higher_priority_wins_within_same_effect() {
        let ir = PolicyIR {
            rules: vec![deny_rule("d1", "x", 10), deny_rule("d2", "x", 100)],
        };
        let rule = PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "x", &facts()).unwrap();
        assert_eq!(rule.rule_id, "d2");
    }

    #[test]
    fn test_wildcard_matches_all() {
        let ir = PolicyIR {
            rules: vec![approval_rule("a1", "*", 50)],
        };
        let rule = PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "anything", &facts()).unwrap();
        assert_eq!(rule.rule_id, "a1");
    }

    #[test]
    fn test_no_match_returns_none() {
        let ir = PolicyIR {
            rules: vec![deny_rule("d1", "shell.exec", 1)],
        };
        assert!(PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "web.fetch", &facts()).is_none());
    }

    #[test]
    fn test_effect_ord_deny_highest() {
        assert!(PolicyEffect::Deny < PolicyEffect::Approval);
        assert!(PolicyEffect::Approval < PolicyEffect::Allow);
    }

    #[test]
    fn test_wildcard_prefix_and_suffix_patterns() {
        let ir = PolicyIR {
            rules: vec![deny_rule("d1", "openai/*", 1)],
        };
        assert!(PolicyPrecedenceEngine::evaluate_matching_rule(
            &ir,
            "openai/gpt-4o",
            &facts()
        )
        .is_some());
        assert!(PolicyPrecedenceEngine::evaluate_matching_rule(
            &ir,
            "zen/deepseek",
            &facts()
        )
        .is_none());

        let suffix_ir = PolicyIR {
            rules: vec![deny_rule("d2", "*.internal", 1)],
        };
        assert!(PolicyPrecedenceEngine::evaluate_matching_rule(
            &suffix_ir,
            "tools.internal",
            &facts()
        )
        .is_some());
        assert!(PolicyPrecedenceEngine::evaluate_matching_rule(
            &suffix_ir,
            "internal.tools",
            &facts()
        )
        .is_none());
    }

    #[test]
    fn test_conditions_gate_rule_matching() {
        // Rule denies only Judge-kind nodes.
        let mut rule = deny_rule("d1", "*", 10);
        rule.conditions = vec![PolicyCondition {
            field: "kind".into(),
            expected: serde_json::json!("Judge"),
        }];
        let ir = PolicyIR { rules: vec![rule] };

        let judge_facts = PolicyFacts {
            model: "m",
            strategy: "Single",
            node_kind: Some("Judge"),
            capability: None,
        };
        let task_facts = PolicyFacts {
            model: "m",
            strategy: "Single",
            node_kind: Some("Task"),
            capability: None,
        };
        assert!(
            PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "any", &judge_facts).is_some()
        );
        assert!(PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "any", &task_facts).is_none());
    }

    #[test]
    fn test_unknown_condition_field_never_matches() {
        let mut rule = deny_rule("d1", "*", 10);
        rule.conditions = vec![PolicyCondition {
            field: "weather".into(),
            expected: serde_json::json!("sunny"),
        }];
        let ir = PolicyIR { rules: vec![rule] };
        assert!(
            PolicyPrecedenceEngine::evaluate_matching_rule(&ir, "any", &facts()).is_none(),
            "unknown condition fields must not silently match"
        );
    }
}
