use std::sync::Arc;
use uuid::Uuid;
use crate::runtime::host_services::CapabilityHostServices;
use crate::runtime::telemetry_context::TelemetryContext;

pub struct RuntimeContext {
    pub execution_id: Uuid,
    pub host_services: Arc<dyn CapabilityHostServices>,
    pub deadline: Option<tokio::time::Instant>,
    pub telemetry: Arc<dyn TelemetryContext>,
}
