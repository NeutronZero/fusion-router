use async_trait::async_trait;
use crate::events::projection::EventProjection;
use crate::events::ExecutionEventEnvelope;
use crate::release::gate::GateError;

#[allow(dead_code)]
pub struct OpenTelemetryProjection;

impl OpenTelemetryProjection {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OpenTelemetryProjection {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventProjection for OpenTelemetryProjection {
    fn name(&self) -> &'static str {
        "OpenTelemetryProjection"
    }

    async fn handle_event(&mut self, envelope: &ExecutionEventEnvelope) -> Result<(), GateError> {
        // Map execution events to tracing::info! spans/events
        tracing::info!(
            target: "fusionrouter::otel",
            event_id = %envelope.event_id,
            workflow_id = %envelope.workflow_id,
            execution_id = %envelope.execution_id,
            seq = %envelope.sequence_number,
            schema = %envelope.schema_version,
            payload = ?envelope.payload,
            "ExecutionEvent"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let projection = OpenTelemetryProjection::new();
        assert_eq!(projection.name(), "OpenTelemetryProjection");
    }

    #[test]
    fn test_default_equivalent_to_new() {
        let default = OpenTelemetryProjection::default();
        assert_eq!(default.name(), OpenTelemetryProjection::new().name());
    }
}
