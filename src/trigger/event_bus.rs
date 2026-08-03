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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_event_creates_event_bus_payload() {
        let payload = EventBusTriggerSubscriber::handle_event(
            "bus-sub-1",
            serde_json::json!({ "event": "deploy.finished", "ref": "main" }),
        );

        assert_eq!(payload.trigger_name, "bus-sub-1");
        assert!(matches!(payload.kind, TriggerKind::EventBus));
        assert_eq!(payload.payload_json["event"], "deploy.finished");
        assert_eq!(payload.payload_json["ref"], "main");
    }

    #[test]
    fn test_handle_event_preserves_arbitrary_data() {
        let payload = EventBusTriggerSubscriber::handle_event(
            "bus-sub-2",
            serde_json::json!([1, 2, 3]),
        );

        assert_eq!(payload.payload_json, serde_json::json!([1, 2, 3]));
    }
}
