//! Phase 7A — `TriggerTrace` & Unified Provenance (`src/trigger/trace.rs`)

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::trigger::types::TriggerKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TriggerEvent {
    RequestReceived {
        request_id: Uuid,
        trigger_kind: TriggerKind,
        trigger_name: String,
        timestamp_ms: u64,
    },
    Deduplicated {
        request_id: Uuid,
        reason: String,
    },
    PipelineDispatched {
        request_id: Uuid,
        plan_id: Uuid,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerTrace {
    pub trace_id: Uuid,
    pub events: Vec<TriggerEvent>,
}

impl TriggerTrace {
    pub fn new() -> Self {
        Self {
            trace_id: Uuid::new_v4(),
            events: Vec::new(),
        }
    }

    pub fn record(&mut self, event: TriggerEvent) {
        self.events.push(event);
    }
}

impl Default for TriggerTrace {
    fn default() -> Self {
        Self::new()
    }
}
