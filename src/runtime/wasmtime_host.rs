use std::sync::Arc;
use async_trait::async_trait;
use reqwest::Request;
use tracing::Level;
use uuid::Uuid;
use crate::capability::CapabilityRegistry;
use crate::events::{bus::EventBus, ExecutionEventEnvelope, payload::ExecutionEvent};
use crate::release::gate::GateError;
use crate::runtime::host_services::CapabilityHostServices;
use crate::runtime::policy::{check_http_access, check_secret_access};
use crate::telemetry::metrics::FusionMetrics;

pub struct WasmtimeCapabilityHost {
    registry: Arc<dyn CapabilityRegistry>,
    event_bus: Arc<dyn EventBus>,
    http_client: reqwest::Client,
    metrics: &'static FusionMetrics,
    execution_id: Uuid,
    workflow_id: Uuid,
    correlation_id: Option<String>,
}

impl WasmtimeCapabilityHost {
    pub fn new(
        registry: Arc<dyn CapabilityRegistry>,
        event_bus: Arc<dyn EventBus>,
        http_client: reqwest::Client,
        metrics: &'static FusionMetrics,
        execution_id: Uuid,
        workflow_id: Uuid,
        correlation_id: Option<String>,
    ) -> Self {
        Self {
            registry,
            event_bus,
            http_client,
            metrics,
            execution_id,
            workflow_id,
            correlation_id,
        }
    }
}

#[async_trait]
impl CapabilityHostServices for WasmtimeCapabilityHost {
    async fn emit_event(&self, event: ExecutionEvent) -> Result<(), GateError> {
        let envelope = ExecutionEventEnvelope::new(
            self.workflow_id.to_string(),
            self.execution_id.to_string(),
            self.correlation_id.clone(),
            0,
            None,
            event,
        );
        self.event_bus.publish(envelope).await
    }

    async fn log(&self, level: Level, message: &str) {
        match level {
            Level::ERROR => tracing::event!(tracing::Level::ERROR, "capability: {}", message),
            Level::WARN => tracing::event!(tracing::Level::WARN, "capability: {}", message),
            Level::INFO => tracing::event!(tracing::Level::INFO, "capability: {}", message),
            Level::DEBUG => tracing::event!(tracing::Level::DEBUG, "capability: {}", message),
            Level::TRACE => tracing::event!(tracing::Level::TRACE, "capability: {}", message),
        }
    }

    async fn fetch_secret(&self, secret_name: &str) -> Result<String, GateError> {
        let contract = self.registry
            .list()
            .into_iter()
            .find(|c| {
                c.permissions.iter().any(|p| matches!(p, fusion_plugin_api::Permission::Secrets(_)))
            })
            .ok_or_else(|| GateError::PermissionDenied("no capability with secret permissions".into()))?;
        check_secret_access(&contract.permissions, secret_name)?;
        std::env::var(secret_name)
            .map_err(|_| GateError::PermissionDenied(format!("secret '{}' not found in environment", secret_name)))
    }

    async fn http_request(&self, req: Request) -> Result<reqwest::Response, GateError> {
        let url_str = req.url().to_string();
        let contract = self.registry
            .list()
            .into_iter()
            .find(|c| {
                c.permissions.iter().any(|p| matches!(p, fusion_plugin_api::Permission::Http(_)))
            })
            .ok_or_else(|| GateError::PermissionDenied("no capability with HTTP permissions".into()))?;
        check_http_access(&contract.permissions, &url_str)?;
        self.http_client.execute(req).await
            .map_err(|e| GateError::ExecutionFailed(format!("HTTP request failed: {e}")))
    }

    fn record_metric(&self, name: &str, value: f64) {
        tracing::info!(metric_name = name, metric_value = value, "capability metric");
        self.metrics.requests_total.inc_by(value as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::InMemoryCapabilityRegistry;
    use crate::events::bus::BroadcastEventBus;
    use std::sync::Arc;
    use fusion_plugin_api::{CapabilityContract, CapabilityId, Permission};

    fn make_registry(permissions: Vec<Permission>) -> Arc<InMemoryCapabilityRegistry> {
        let mut reg = InMemoryCapabilityRegistry::new();
        let contract = CapabilityContract {
            id: CapabilityId::new("test.cap"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "test".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions,
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        };
        reg.register(contract).unwrap();
        Arc::new(reg)
    }

    #[tokio::test]
    async fn test_emit_event_publishes_to_event_bus() {
        let bus = Arc::new(BroadcastEventBus::new(16));
        let mut rx = bus.subscribe();
        let host = WasmtimeCapabilityHost::new(
            make_registry(vec![]),
            bus,
            reqwest::Client::new(),
            FusionMetrics::instance(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
        );
        host.emit_event(ExecutionEvent::WorkflowStarted {
            intent: "test".into(),
            input_tokens: 10,
        }).await.unwrap();
        let received = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            rx.recv(),
        ).await;
        assert!(received.is_ok(), "expected event on bus");
    }

    #[tokio::test]
    async fn test_fetch_secret_permission_denied_without_secrets_perm() {
        let host = WasmtimeCapabilityHost::new(
            make_registry(vec![Permission::Network]),
            Arc::new(BroadcastEventBus::new(16)),
            reqwest::Client::new(),
            FusionMetrics::instance(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
        );
        let result = host.fetch_secret("db_password").await;
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[tokio::test]
    async fn test_http_request_permission_denied_without_http_perm() {
        let host = WasmtimeCapabilityHost::new(
            make_registry(vec![Permission::Secrets("x".into())]),
            Arc::new(BroadcastEventBus::new(16)),
            reqwest::Client::new(),
            FusionMetrics::instance(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
        );
        let req = reqwest::Request::new(reqwest::Method::GET, "https://example.com".parse().unwrap());
        let result = host.http_request(req).await;
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[tokio::test]
    async fn test_log_does_not_panic() {
        let host = WasmtimeCapabilityHost::new(
            make_registry(vec![]),
            Arc::new(BroadcastEventBus::new(16)),
            reqwest::Client::new(),
            FusionMetrics::instance(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
        );
        host.log(tracing::Level::INFO, "hello from capability").await;
    }

    #[test]
    fn test_record_metric_does_not_panic() {
        let host = WasmtimeCapabilityHost::new(
            make_registry(vec![]),
            Arc::new(BroadcastEventBus::new(16)),
            reqwest::Client::new(),
            FusionMetrics::instance(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
        );
        host.record_metric("test.metric", 42.0);
    }
}
