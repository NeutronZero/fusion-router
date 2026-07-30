use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use reqwest::{Request, Response};
use tracing::Level;

use crate::events::payload::ExecutionEvent;
use crate::release::gate::GateError;
use crate::runtime::host_services::CapabilityHostServices;

pub struct MockHostServices {
    pub emitted_events: Arc<Mutex<Vec<ExecutionEvent>>>,
    pub logs: Arc<Mutex<Vec<(Level, String)>>>,
    pub secrets: HashMap<String, String>,
    pub http_responses: HashMap<String, (u16, Vec<u8>)>,
    pub metrics: Arc<Mutex<Vec<(String, f64)>>>,
}

impl MockHostServices {
    pub fn new(secrets: HashMap<String, String>, http_responses: HashMap<String, (u16, Vec<u8>)>) -> Self {
        Self {
            emitted_events: Arc::new(Mutex::new(Vec::new())),
            logs: Arc::new(Mutex::new(Vec::new())),
            secrets,
            http_responses,
            metrics: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Default for MockHostServices {
    fn default() -> Self {
        Self::new(HashMap::new(), HashMap::new())
    }
}

#[async_trait]
impl CapabilityHostServices for MockHostServices {
    async fn emit_event(&self, event: ExecutionEvent) -> Result<(), GateError> {
        self.emitted_events.lock().push(event);
        Ok(())
    }

    async fn log(&self, level: Level, message: &str) {
        self.logs.lock().push((level, message.to_string()));
    }

    async fn fetch_secret(&self, secret_name: &str) -> Result<String, GateError> {
        self.secrets
            .get(secret_name)
            .cloned()
            .ok_or_else(|| GateError::ExecutionFailed(format!("secret not found: {secret_name}")))
    }

    async fn http_request(&self, req: Request) -> Result<Response, GateError> {
        let url = req.url().to_string();
        let (status, body) = self
            .http_responses
            .get(&url)
            .cloned()
            .ok_or_else(|| GateError::ExecutionFailed(format!("no mock response for: {url}")))?;

        let http_resp = http::Response::builder()
            .status(status)
            .body(body)
            .map_err(|e| GateError::ExecutionFailed(format!("response build error: {e}")))?;

        Ok(Response::from(http_resp))
    }

    fn record_metric(&self, name: &str, value: f64) {
        self.metrics.lock().push((name.to_string(), value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_emit_event_records_event() {
        let host = MockHostServices::default();
        let event = ExecutionEvent::NodeStarted {
            node_id: "test-node".into(),
            target_model: None,
        };
        host.emit_event(event).await.unwrap();
        let events = host.emitted_events.lock();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ExecutionEvent::NodeStarted { .. }));
    }

    #[tokio::test]
    async fn test_log_records_entry() {
        let host = MockHostServices::default();
        host.log(Level::INFO, "hello").await;
        let logs = host.logs.lock();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].1, "hello");
    }

    #[tokio::test]
    async fn test_metric_records_value() {
        let host = MockHostServices::default();
        host.record_metric("test.counter", 42.0);
        let metrics = host.metrics.lock();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0], ("test.counter".to_string(), 42.0));
    }

    #[tokio::test]
    async fn test_fetch_secret_returns_configured_value() {
        let mut secrets = HashMap::new();
        secrets.insert("DB_PASSWORD".into(), "supersecret".into());
        let host = MockHostServices::new(secrets, HashMap::new());
        let val = host.fetch_secret("DB_PASSWORD").await.unwrap();
        assert_eq!(val, "supersecret");
    }

    #[tokio::test]
    async fn test_fetch_secret_missing_returns_error() {
        let host = MockHostServices::default();
        let result = host.fetch_secret("NONEXISTENT").await;
        assert!(result.is_err());
    }
}
