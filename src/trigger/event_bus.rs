//! Phase 6C — `EventBusTriggerSubscriber` (`src/trigger/event_bus.rs`)

use crate::trigger::types::{TriggerKind, TriggerPayload};

pub struct EventBusTriggerSubscriber;

impl EventBusTriggerSubscriber {
    /// Subscribes and converts event bus messages into a TriggerPayload.
    pub fn handle_event(
        trigger_name: impl Into<String>,
        event_data: serde_json::Value,
    ) -> TriggerPayload {
        TriggerPayload {
            trigger_name: trigger_name.into(),
            kind: TriggerKind::EventBus,
            payload_json: event_data,
        }
    }
}
