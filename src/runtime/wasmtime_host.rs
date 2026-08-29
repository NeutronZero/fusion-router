use crate::capability::CapabilityRegistry;
use crate::events::{bus::EventBus, payload::ExecutionEvent, ExecutionEventEnvelope};
use crate::release::gate::GateError;
use crate::runtime::host_services::CapabilityHostServices;
use crate::runtime::policy::{check_http_access, check_secret_access};
use crate::telemetry::metrics::FusionMetrics;
use crate::transport::http::build_ssrf_hardened_client;
use async_trait::async_trait;
use reqwest::Request;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use tracing::Level;
use uuid::Uuid;

pub struct WasmtimeCapabilityHost {
    registry: Arc<dyn CapabilityRegistry>,
    event_bus: Arc<dyn EventBus>,
    http_client: reqwest::Client,
    metrics: &'static FusionMetrics,
    execution_id: Uuid,
    workflow_id: Uuid,
    correlation_id: Option<String>,
    /// Caller-bound permissions (ADR-036). When set, host functions enforce
    /// against this list only — registry is not scanned (fixes C1 confused
    /// deputy). When None, behavior falls back to deny (fail-closed) rather
    /// than scanning the whole registry.
    caller_permissions: Option<Vec<fusion_plugin_api::Permission>>,
    caller_id: Option<fusion_plugin_api::CapabilityId>,
}

impl WasmtimeCapabilityHost {
    pub fn new(
        registry: Arc<dyn CapabilityRegistry>,
        event_bus: Arc<dyn EventBus>,
        metrics: &'static FusionMetrics,
        execution_id: Uuid,
        workflow_id: Uuid,
        correlation_id: Option<String>,
    ) -> Self {
        // SSRF-hardened client: redirects disabled and a dial-time validating
        // DNS resolver that rejects loopback/private/link-local addresses at
        // connect time, closing the DNS-rebinding TOCTOU window. Fail-closed:
        // if the hardened client cannot be built we panic rather than run with
        // a default (redirect-following, non-validating) client.
        let http_client = build_ssrf_hardened_client()
            .expect("WasmtimeCapabilityHost: failed to build SSRF-hardened HTTP client");
        Self {
            registry,
            event_bus,
            http_client,
            metrics,
            execution_id,
            workflow_id,
            correlation_id,
            caller_permissions: None,
            caller_id: None,
        }
    }

    /// Create a host bound to a specific capability's permissions (ADR-036).
    /// All `fetch_secret` / `http_request` checks are scoped to `contract.permissions`.
    pub fn with_caller(
        registry: Arc<dyn CapabilityRegistry>,
        event_bus: Arc<dyn EventBus>,
        metrics: &'static FusionMetrics,
        execution_id: Uuid,
        workflow_id: Uuid,
        correlation_id: Option<String>,
        caller_contract: fusion_plugin_api::CapabilityContract,
    ) -> Self {
        let perms = caller_contract.permissions.clone();
        let id = caller_contract.id.clone();
        let http_client = build_ssrf_hardened_client()
            .expect("WasmtimeCapabilityHost: failed to build SSRF-hardened HTTP client");
        Self {
            registry,
            event_bus,
            http_client,
            metrics,
            execution_id,
            workflow_id,
            correlation_id,
            caller_permissions: Some(perms),
            caller_id: Some(id),
        }
    }

    /// Override caller permissions directly (testing / non-contract callers).
    pub fn with_caller_permissions(
        mut self,
        perms: Vec<fusion_plugin_api::Permission>,
    ) -> Self {
        self.caller_permissions = Some(perms);
        self
    }

    fn effective_permissions(&self) -> Result<Vec<fusion_plugin_api::Permission>, GateError> {
        if let Some(perms) = &self.caller_permissions {
            return Ok(perms.clone());
        }
        // Fail-closed: no caller identity -> no permissions. Previously this
        // scanned the registry for ANY contract holding the permission (C1).
        Err(GateError::PermissionDenied(
            "no caller identity bound to host — call denied (ADR-036)".into(),
        ))
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
        if secret_name.is_empty() || secret_name == "*" {
            return Err(GateError::PermissionDenied(
                "secret name must not be empty or '*'".into(),
            ));
        }
        let perms = self.effective_permissions()?;
        check_secret_access(&perms, secret_name)?;
        std::env::var(secret_name).map_err(|_| {
            GateError::PermissionDenied(format!(
                "secret '{}' not found in environment",
                secret_name
            ))
        })
    }

    async fn http_request(&self, req: Request) -> Result<reqwest::Response, GateError> {
        let perms = self.effective_permissions()?;
        let url_str = req.url().to_string();
        check_http_access(&perms, &url_str)?;
        validate_ssrf(req.url())
            .map_err(|m| GateError::PermissionDenied(format!("SSRF guard: {m}")))?;
        let response = self
            .http_client
            .execute(req)
            .await
            .map_err(|e| GateError::ExecutionFailed(format!("HTTP request failed: {e}")))?;
        validate_ssrf(response.url())
            .map_err(|m| GateError::PermissionDenied(format!("SSRF guard (final): {m}")))?;
        if let Some(location) = response.headers().get(reqwest::header::LOCATION) {
            if let Ok(loc_str) = location.to_str() {
                if let Ok(loc_url) = reqwest::Url::parse(loc_str) {
                    validate_ssrf(&loc_url).map_err(|m| {
                        GateError::PermissionDenied(format!("SSRF guard (redirect): {m}"))
                    })?;
                }
            }
        }
        Ok(response)
    }

    fn record_metric(&self, name: &str, value: f64) {
        // Reject non-finite and negative values: a negative f64 would be cast
        // into an enormous u64 and corrupt the counter.
        let v = if value.is_finite() { value.max(0.0) } else { 0.0 };
        tracing::info!(
            metric_name = name,
            metric_value = v,
            "capability metric"
        );
        self.metrics.requests_total.inc_by(v as u64);
    }
}

fn is_blocked_ip(ip: &std::net::IpAddr) -> bool {
    let ip = match ip {
        std::net::IpAddr::V6(v6) => v6.to_ipv4_mapped().map(std::net::IpAddr::V4).unwrap_or(*ip),
        other => *other,
    };
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_link_local()
                || v4.is_private()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xC0) == 0x40)
        }
        std::net::IpAddr::V6(v6) => {
            let s = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] == 0x0064 && s[1] == 0xff9b && s[2..6].iter().all(|&seg| seg == 0))
                || (s[..6].iter().all(|&seg| seg == 0) && (s[6] != 0 || s[7] != 0))
        }
    }
}

fn validate_ssrf(url: &reqwest::Url) -> Result<(), String> {
    if url.scheme().to_ascii_lowercase() != "https" {
        return Err(format!(
            "URL scheme '{}' is not allowed (https only)",
            url.scheme()
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| "URL must have a host".to_string())?;
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if is_blocked_ip(&ip) {
            return Err(format!("URL host '{host}' resolves to a blocked address"));
        }
    } else {
        let port = url.port_or_known_default().unwrap_or(443);
        let addrs: Vec<std::net::SocketAddr> = (host, port)
            .to_socket_addrs()
            .map_err(|e| format!("DNS resolution failed for '{host}': {e}"))?
            .collect();
        if addrs.is_empty() {
            return Err(format!("URL host '{host}' resolved to no addresses"));
        }
        for addr in &addrs {
            if is_blocked_ip(&addr.ip()) {
                return Err(format!(
                    "URL host '{host}' resolves to blocked address {}",
                    addr.ip()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::InMemoryCapabilityRegistry;
    use crate::events::bus::BroadcastEventBus;
    use fusion_plugin_api::{CapabilityContract, CapabilityId, Permission};
    use std::sync::Arc;

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
            estimated_cost: fusion_core::NanoUSD::ZERO,
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
            FusionMetrics::instance(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
        );
        host.emit_event(ExecutionEvent::WorkflowStarted {
            intent: "test".into(),
            input_tokens: 10,
        })
        .await
        .unwrap();
        let received = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await;
        assert!(received.is_ok(), "expected event on bus");
    }

    #[tokio::test]
    async fn test_fetch_secret_permission_denied_without_secrets_perm() {
        let host = WasmtimeCapabilityHost::new(
            make_registry(vec![Permission::Network]),
            Arc::new(BroadcastEventBus::new(16)),
            FusionMetrics::instance(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
        )
        .with_caller_permissions(vec![Permission::Network]);
        let result = host.fetch_secret("db_password").await;
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[tokio::test]
    async fn test_http_request_permission_denied_without_http_perm() {
        let host = WasmtimeCapabilityHost::new(
            make_registry(vec![Permission::Secrets("x".into())]),
            Arc::new(BroadcastEventBus::new(16)),
            FusionMetrics::instance(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
        )
        .with_caller_permissions(vec![Permission::Secrets("x".into())]);
        let req =
            reqwest::Request::new(reqwest::Method::GET, "https://example.com".parse().unwrap());
        let result = host.http_request(req).await;
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
    }

    #[tokio::test]
    async fn test_fetch_secret_denied_without_caller_identity_even_if_registry_has_perm() {
        // C1 confused deputy: registry has broad Secrets perm but caller has none.
        let mut reg = InMemoryCapabilityRegistry::new();
        let broad = CapabilityContract {
            id: CapabilityId::new("broad.cap"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "broad".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![Permission::Secrets("API_KEY".into())],
            dependencies: vec![],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        };
        reg.register(broad).unwrap();
        let registry = Arc::new(reg);
        // caller has no Secrets perm -> must deny even though registry contains it
        let host = WasmtimeCapabilityHost::new(
            registry,
            Arc::new(BroadcastEventBus::new(16)),
            FusionMetrics::instance(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
        )
        .with_caller_permissions(vec![Permission::Network]);
        let result = host.fetch_secret("API_KEY").await;
        assert!(
            matches!(result, Err(GateError::PermissionDenied(_))),
            "confused deputy: caller without Secrets must not inherit registry's broad perm"
        );
    }

    #[tokio::test]
    async fn test_fetch_secret_allowed_with_caller_perm() {
        let host = WasmtimeCapabilityHost::new(
            make_registry(vec![Permission::Secrets("TEST_SECRET_FOO".into())]),
            Arc::new(BroadcastEventBus::new(16)),
            FusionMetrics::instance(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
        )
        .with_caller_permissions(vec![Permission::Secrets("TEST_SECRET_FOO".into())]);
        // env var not set -> PermissionDenied with "not found in environment"
        let result = host.fetch_secret("TEST_SECRET_FOO").await;
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
        assert!(result.unwrap_err().to_string().contains("not found in environment"));
    }

    #[tokio::test]
    async fn test_no_caller_identity_is_fail_closed() {
        let host = WasmtimeCapabilityHost::new(
            make_registry(vec![Permission::Secrets("API_KEY".into())]),
            Arc::new(BroadcastEventBus::new(16)),
            FusionMetrics::instance(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
        );
        let result = host.fetch_secret("API_KEY").await;
        assert!(matches!(result, Err(GateError::PermissionDenied(_))));
        assert!(result.unwrap_err().to_string().contains("no caller identity"));
    }

    #[tokio::test]
    async fn test_log_does_not_panic() {
        let host = WasmtimeCapabilityHost::new(
            make_registry(vec![]),
            Arc::new(BroadcastEventBus::new(16)),
            FusionMetrics::instance(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
        );
        host.log(tracing::Level::INFO, "hello from capability")
            .await;
    }

    #[test]
    fn test_record_metric_does_not_panic() {
        let host = WasmtimeCapabilityHost::new(
            make_registry(vec![]),
            Arc::new(BroadcastEventBus::new(16)),
            FusionMetrics::instance(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
        );
        host.record_metric("test.metric", 42.0);
    }
}
