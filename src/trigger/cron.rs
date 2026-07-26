//! Phase 6C — `CronTriggerScheduler` (`src/trigger/cron.rs`)

use crate::trigger::types::{TriggerKind, TriggerPayload};

pub struct CronTriggerScheduler;

impl CronTriggerScheduler {
    /// Generates a scheduled TriggerPayload on cron timer trigger.
    pub fn trigger_scheduled(
        trigger_name: impl Into<String>,
        schedule: impl Into<String>,
    ) -> TriggerPayload {
        TriggerPayload {
            trigger_name: trigger_name.into(),
            kind: TriggerKind::Cron,
            payload_json: serde_json::json!({
                "schedule": schedule.into(),
                "triggered_at_ms": 1000,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_trigger_scheduler() {
        let payload = CronTriggerScheduler::trigger_scheduled("nightly-clean", "0 0 * * *");
        assert_eq!(payload.kind, TriggerKind::Cron);
    }
}
