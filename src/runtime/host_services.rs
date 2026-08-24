use crate::events::payload::ExecutionEvent;
use crate::release::gate::GateError;
use async_trait::async_trait;

#[async_trait]
pub trait CapabilityHostServices: Send + Sync {
    async fn emit_event(&self, event: ExecutionEvent) -> Result<(), GateError>;
    async fn log(&self, level: tracing::Level, message: &str);
    async fn fetch_secret(&self, secret_name: &str) -> Result<String, GateError>;
    async fn http_request(&self, req: reqwest::Request) -> Result<reqwest::Response, GateError>;
    fn record_metric(&self, name: &str, value: f64);
}
