//! Phase 4B1 — `PolicyTrace` & Policy Match Event Log (`src/policy/trace.rs`)
//!
//! Provenance tracing capturing policy match events and graph transformations.

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::policy::ir::PolicyEffect;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PolicyMatchEvent {
    RuleMatched {
        node_id: Uuid,
        rule_id: String,
        target_pattern: String,
        effect: PolicyEffect,
    },
    NodeInserted {
        inserted_node_id: Uuid,
        node_kind: String,
        target_node_id: Uuid,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyTrace {
    pub trace_id: Uuid,
    pub events: Vec<PolicyMatchEvent>,
}

impl PolicyTrace {
    pub fn new() -> Self {
        Self {
            trace_id: Uuid::new_v4(),
            events: Vec::new(),
        }
    }

    pub fn record(&mut self, event: PolicyMatchEvent) {
        self.events.push(event);
    }
}

impl Default for PolicyTrace {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::ir::PolicyEffect;

    #[test]
    fn test_new_trace_is_empty() {
        let trace = PolicyTrace::new();
        assert!(trace.events.is_empty());
    }

    #[test]
    fn test_record_appends_events_in_order() {
        let mut trace = PolicyTrace::new();
        let node_id = Uuid::new_v4();
        let target_id = Uuid::new_v4();

        trace.record(PolicyMatchEvent::RuleMatched {
            node_id,
            rule_id: "rule-1".into(),
            target_pattern: "llm/**".into(),
            effect: PolicyEffect::Allow,
        });
        trace.record(PolicyMatchEvent::NodeInserted {
            inserted_node_id: node_id,
            node_kind: "LLMGenerate".into(),
            target_node_id: target_id,
        });

        assert_eq!(trace.events.len(), 2);
        match &trace.events[0] {
            PolicyMatchEvent::RuleMatched {
                node_id: n,
                rule_id,
                target_pattern,
                effect,
            } => {
                assert_eq!(*n, node_id);
                assert_eq!(rule_id, "rule-1");
                assert_eq!(target_pattern, "llm/**");
                assert_eq!(*effect, PolicyEffect::Allow);
            }
            _ => panic!("expected RuleMatched event"),
        }
        match &trace.events[1] {
            PolicyMatchEvent::NodeInserted {
                inserted_node_id,
                node_kind,
                target_node_id,
            } => {
                assert_eq!(*inserted_node_id, node_id);
                assert_eq!(node_kind, "LLMGenerate");
                assert_eq!(*target_node_id, target_id);
            }
            _ => panic!("expected NodeInserted event"),
        }
    }

    #[test]
    fn test_default_matches_new() {
        let default = PolicyTrace::default();
        assert_eq!(default.events.len(), PolicyTrace::new().events.len());
        assert!(!default.trace_id.is_nil());
    }
}
