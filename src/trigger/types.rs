//! Phase 6B — `TriggerKind`, `TriggerDeclaration`, & `TriggerPayload` (`src/trigger/types.rs`)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerKind {
    Webhook,
    Cron,
    EventBus,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerDeclaration {
    pub name: String,
    pub kind: TriggerKind,
    pub endpoint_or_schedule: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerPayload {
    pub trigger_name: String,
    pub kind: TriggerKind,
    pub payload_json: serde_json::Value,
}

/// Canonical ExecutionRequest structure enforcing ADR-031 invariants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRequest {
    pub request_id: uuid::Uuid,
    pub trigger_kind: TriggerKind,
    pub trigger_name: String,
    pub payload: serde_json::Value,
    pub requester_identity: String,
    pub created_at_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_trigger_types() {
        let decl = TriggerDeclaration {
            name: "github-webhook".into(),
            kind: TriggerKind::Webhook,
            endpoint_or_schedule: "/api/webhooks/github".into(),
            headers: HashMap::new(),
        };

        let payload = TriggerPayload {
            trigger_name: decl.name.clone(),
            kind: TriggerKind::Webhook,
            payload_json: json!({"action": "opened"}),
        };

        assert_eq!(payload.kind, TriggerKind::Webhook);
    }
}
