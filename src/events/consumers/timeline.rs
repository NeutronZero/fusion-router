use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::events::projection::EventProjection;
use crate::events::ExecutionEventEnvelope;
use crate::release::gate::GateError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub sequence_number: u64,
    pub relative_ms: u64,
    pub event_type: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimelineModel {
    pub execution_id: String,
    pub start_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub entries: Vec<TimelineEntry>,
}

impl TimelineModel {
    pub fn render_ascii(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Workflow Timeline: {}\n\n", self.execution_id));
        for entry in &self.entries {
            out.push_str(&format!(
                "{:03}ms ├─► [{}] {}\n",
                entry.relative_ms, entry.event_type, entry.summary
            ));
        }
        out
    }
}

pub struct TimelineProjection {
    pub model: TimelineModel,
}

impl TimelineProjection {
    pub fn new(execution_id: impl Into<String>) -> Self {
        Self {
            model: TimelineModel {
                execution_id: execution_id.into(),
                start_timestamp: None,
                entries: Vec::new(),
            },
        }
    }
}

#[async_trait]
impl EventProjection for TimelineProjection {
    fn name(&self) -> &'static str {
        "TimelineProjection"
    }

    async fn handle_event(&mut self, envelope: &ExecutionEventEnvelope) -> Result<(), GateError> {
        let start = *self.model.start_timestamp.get_or_insert(envelope.timestamp);
        let relative_ms = envelope
            .timestamp
            .signed_duration_since(start)
            .num_milliseconds()
            .max(0) as u64;

        let event_type = format!("{:?}", envelope.payload);
        let type_name = event_type.split('{').next().unwrap_or("Event").trim().to_string();

        self.model.entries.push(TimelineEntry {
            sequence_number: envelope.sequence_number,
            relative_ms,
            event_type: type_name,
            summary: format!("{:?}", envelope.payload),
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::payload::ExecutionEvent;

    #[tokio::test]
    async fn test_timeline_projection_render_ascii() {
        let mut proj = TimelineProjection::new("exec-100");
        let env = ExecutionEventEnvelope::new(
            "wf-1",
            "exec-100",
            None,
            1,
            None,
            ExecutionEvent::WorkflowStarted {
                intent: "Quality".into(),
                input_tokens: 100,
            },
        );

        proj.handle_event(&env).await.unwrap();
        let ascii = proj.model.render_ascii();
        assert!(ascii.contains("Workflow Timeline: exec-100"));
        assert!(ascii.contains("WorkflowStarted"));
    }
}
