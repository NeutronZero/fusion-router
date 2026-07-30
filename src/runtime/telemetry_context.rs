use uuid::Uuid;

pub trait TelemetryContext: Send + Sync {
    fn execution_id(&self) -> &Uuid;
    fn record_counter(&self, name: &str, value: u64);
}
