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
