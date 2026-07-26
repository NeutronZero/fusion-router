//! Phase 6C — `WebhookTriggerHandler` (`src/trigger/webhook.rs`)

use crate::trigger::types::{TriggerKind, TriggerPayload};

pub struct WebhookTriggerHandler;

impl WebhookTriggerHandler {
    /// Handles an incoming HTTP webhook request and packages it into a TriggerPayload.
    pub fn process_webhook(
        trigger_name: impl Into<String>,
        body: serde_json::Value,
    ) -> TriggerPayload {
        TriggerPayload {
            trigger_name: trigger_name.into(),
            kind: TriggerKind::Webhook,
            payload_json: body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webhook_trigger_handler() {
        let payload = WebhookTriggerHandler::process_webhook(
            "github-issue-opened",
            serde_json::json!({"issue": 42}),
        );

        assert_eq!(payload.trigger_name, "github-issue-opened");
        assert_eq!(payload.kind, TriggerKind::Webhook);
    }
}
