use crate::scheduler::connector_resolver::{Connector, ConnectorDescriptor};
use crate::transport::http::is_blocked_ip;
use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance, CapabilityPlugin,
    ExecutionError, ExecutionResult, Permission, Plugin, PluginMetadata,
};
use serde_json::json;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use futures::StreamExt;

/// Maximum response body size kept in `outputs.body` (bytes).
const MAX_BODY_BYTES: usize = 64 * 1024;

async fn validate_url_for_ssrf(url_str: &str) -> Result<(), String> {
    let url = url::Url::parse(url_str).map_err(|e| format!("invalid URL: {e}"))?;
    let scheme = url.scheme().to_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!("URL scheme '{scheme}' not allowed (use http or https)"));
    }
    let host_str = url
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;
    // Block obvious loopback hostnames
    if host_str == "localhost"
        || host_str == "127.0.0.1"
        || host_str == "::1"
        || host_str == "0.0.0.0"
    {
        return Err("URL targets loopback address".to_string());
    }
    if let Ok(ip) = host_str.parse::<IpAddr>() {
        if is_blocked_ip(&ip) {
            return Err(format!("URL targets reserved IP address {ip}"));
        }
        return Ok(());
    }
    let port = url.port_or_known_default().unwrap_or(80);
    let addrs = tokio::net::lookup_host((host_str, port))
        .await
        .map_err(|e| format!("DNS resolution failed for '{host_str}': {e}"))?;
    for addr in addrs {
        if is_blocked_ip(&addr.ip()) {
            return Err(format!(
                "URL host '{host_str}' resolves to blocked address {}",
                addr.ip()
            ));
        }
    }
    Ok(())
}

async fn read_body_limited(response: reqwest::Response) -> Result<String, String> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("failed to read response body: {e}"))?;
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            let remaining = MAX_BODY_BYTES.saturating_sub(body.len());
            body.extend_from_slice(&chunk[..remaining]);
            break;
        }
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).to_string())
}

/// Redacts a URL for safe error messages (keeps scheme + host only).
fn redact_url(url_str: &str) -> String {
    match url::Url::parse(url_str) {
        Ok(u) => {
            let host = u.host_str().unwrap_or("???");
            let path = if u.path() == "/" { "" } else { "..." };
            format!("{}://{host}{path}", u.scheme())
        }
        Err(_) => "<invalid-url>".to_string(),
    }
}

/// Makes real HTTP requests (GET/POST) via `reqwest`.
pub struct HttpPlugin {
    client: reqwest::Client,
}

impl Default for HttpPlugin {
    fn default() -> Self {
        // SSRF-hardened client: redirects disabled (no pivot to an internal
        // target) and a dial-time validating DNS resolver that rejects
        // loopback/private/link-local addresses at connect time, closing the
        // DNS-rebinding TOCTOU window. Fail-closed: if the hardened client
        // cannot be built we panic rather than silently fall back to a
        // redirect-following, non-validating default client.
        let client = crate::transport::http::build_ssrf_hardened_client()
            .expect("HttpPlugin: failed to build SSRF-hardened HTTP client");
        Self { client }
    }
}

impl Plugin for HttpPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "fusion-connector-http".into(),
            version: semver::Version::new(0, 1, 0),
            api_version: semver::Version::new(0, 1, 0),
            min_compiler_version: semver::Version::new(0, 9, 0),
            capabilities: vec![CapabilityId::new("http.request")],
        }
    }
}

impl CapabilityPlugin for HttpPlugin {
    fn capabilities(&self) -> Vec<CapabilityContract> {
        vec![CapabilityContract {
            id: CapabilityId::new("http.request"),
            version: semver::Version::new(0, 1, 0),
            description: "Makes an HTTP request".into(),
            inputs_schema: json!({"type": "object", "properties": {"url": {"type": "string"}, "method": {"type": "string"}}}),
            outputs_schema: json!({"type": "object", "properties": {"body": {"type": "string"}, "status": {"type": "number"}}}),
            permissions: vec![Permission::Network],
            dependencies: vec![],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 150,
            reliability_score: 0.99,
            supports_streaming: false,
            traits: vec![],
        }]
    }
}

#[async_trait]
impl CapabilityExecutor for HttpPlugin {
    async fn execute(
        &self,
        instance: &CapabilityInstance,
        input: serde_json::Value,
    ) -> Result<ExecutionResult, ExecutionError> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutionError {
                connector: "http".into(),
                capability: instance.contract.id.clone(),
                reason: "Missing 'url' field".into(),
                retryable: false,
            })?;

        // SSRF protection: reject loopback, private, and link-local addresses.
        // This is the URL-layer gate (eager, precise errors); the hardened
        // client's dial-time resolver independently re-validates the address
        // it actually dials, so DNS changes between this check and connect
        // cannot pivot the request to an internal target.
        validate_url_for_ssrf(url).await.map_err(|msg| ExecutionError {
            connector: "http".into(),
            capability: instance.contract.id.clone(),
            reason: msg,
            retryable: false,
        })?;

        let perms = &instance.contract.permissions;
        let permitted = perms
            .iter()
            .any(|p| matches!(p, Permission::Network))
            || crate::runtime::policy::check_http_access(perms, url).is_ok();
        if !perms.is_empty() && !permitted {
            return Err(ExecutionError {
                connector: "http".into(),
                capability: instance.contract.id.clone(),
                reason: "request URL is not covered by the connector's declared permissions".into(),
                retryable: false,
            });
        }

        let _method = input
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET");
        let method = _method.to_uppercase();
        let body = input.get("body").and_then(|v| v.as_str()).unwrap_or("");

        let started = std::time::Instant::now();
        let mut request = self.client.request(
            reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| ExecutionError {
                connector: "http".into(),
                capability: instance.contract.id.clone(),
                reason: format!("unsupported HTTP method: {method}"),
                retryable: false,
            })?,
            url,
        );
        if !body.is_empty() {
            request = request.body(body.to_string());
        }

        let response = request.send().await.map_err(|err| ExecutionError {
            connector: "http".into(),
            capability: instance.contract.id.clone(),
            reason: format!("request to {} failed: {err}", redact_url(url)),
            retryable: true,
        })?;

        let status = response.status().as_u16();
        let text = read_body_limited(response).await.map_err(|err| ExecutionError {
            connector: "http".into(),
            capability: instance.contract.id.clone(),
            reason: err,
            retryable: false,
        })?;

        let mut metrics = HashMap::new();
        metrics.insert(
            "latency_ms".to_string(),
            started.elapsed().as_secs_f64() * 1000.0,
        );

        let truncated = text;

        Ok(ExecutionResult {
            outputs: json!({ "body": truncated, "status": status }),
            metrics,
        })
    }
}

pub struct HttpConnector {
    plugin: Arc<HttpPlugin>,
}

impl HttpConnector {
    pub fn new() -> Self {
        Self {
            plugin: Arc::new(HttpPlugin::default()),
        }
    }
}

impl Default for HttpConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl Connector for HttpConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            name: "http".into(),
            version: semver::Version::new(0, 10, 0),
            supported_capabilities: vec![CapabilityId::new("http.request")],
        }
    }

    fn executor(&self) -> Arc<dyn CapabilityExecutor> {
        self.plugin.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_plugin_api::CapabilityContract;

    fn make_instance() -> CapabilityInstance {
        CapabilityInstance {
            contract: CapabilityContract {
                id: CapabilityId::new("http.request"),
                version: semver::Version::parse("0.1.0").unwrap(),
                description: "test".into(),
                inputs_schema: json!({}),
                outputs_schema: json!({}),
                permissions: vec![],
                dependencies: vec![],
                estimated_cost: fusion_core::NanoUSD::ZERO,
                estimated_latency_ms: 1,
                reliability_score: 1.0,
                supports_streaming: false,
                traits: vec![],
            },
            runtime_params: json!({}),
        }
    }

    #[test]
    fn test_http_connector_descriptor() {
        let connector = HttpConnector::new();
        let desc = connector.descriptor();
        assert_eq!(desc.name, "http");
        assert_eq!(desc.supported_capabilities.len(), 1);
    }

    #[tokio::test]
    async fn test_makes_request_to_public_host() {
        // Safe test URL (public, non-internal) exercised through the hardened
        // client (redirects disabled + dial-time validating resolver).
        let plugin = HttpPlugin::default();
        let result = plugin
            .execute(
                &make_instance(),
                json!({ "url": "https://example.com" }),
            )
            .await;
        assert!(
            result.is_ok(),
            "public host request must succeed through hardened client: {:?}",
            result.err()
        );
        let result = result.unwrap();
        assert_eq!(result.outputs["status"], 200);
    }

    #[tokio::test]
    async fn test_ssrf_blocks_link_local_metadata_address() {
        // The SSRF gate must reject internal targets (here the cloud metadata
        // endpoint) rather than fetching them. This is hermetic — validation
        // happens before any network dial.
        let plugin = HttpPlugin::default();
        let err = plugin
            .execute(
                &make_instance(),
                json!({ "url": "http://169.254.169.254/latest/meta-data" }),
            )
            .await
            .unwrap_err();
        assert!(
            err.reason.to_lowercase().contains("blocked")
                || err.reason.to_lowercase().contains("reserved"),
            "internal metadata address must be blocked, got: {}",
            err.reason
        );
        // SSRF rejections are permanent, not transient/retryable.
        assert!(!err.retryable);
    }
}
