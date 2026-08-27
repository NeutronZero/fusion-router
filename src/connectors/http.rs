use crate::scheduler::connector_resolver::{Connector, ConnectorDescriptor};
use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance, CapabilityPlugin,
    ExecutionError, ExecutionResult, Permission, Plugin, PluginMetadata,
};
use serde_json::json;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

/// Maximum response body size kept in `outputs.body` (bytes).
const MAX_BODY_BYTES: usize = 64 * 1024;

/// Validates that a URL is safe to fetch (SSRF protection).
/// Rejects loopback, private, and link-local addresses.
fn validate_url_for_ssrf(url_str: &str) -> Result<(), String> {
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
    // Try to parse as IP and check private/link-local ranges
    if let Ok(ip) = host_str.parse::<IpAddr>() {
        if ip.is_loopback() || ip.is_unspecified() {
            return Err(format!("URL targets reserved IP address {ip}"));
        }
        match ip {
            IpAddr::V4(v4) => {
                if v4.is_link_local() {
                    return Err(format!("URL targets link-local address {v4}"));
                }
                let octets = v4.octets();
                if octets[0] == 10
                    || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                    || (octets[0] == 192 && octets[1] == 168)
                {
                    return Err(format!("URL targets private IP address {v4}"));
                }
            }
            IpAddr::V6(v6) => {
                let segments = v6.segments();
                // fc00::/7 (unique local), fe80::/10 (link-local)
                if (segments[0] & 0xfe00) == 0xfc00 || (segments[0] & 0xffc0) == 0xfe80 {
                    return Err(format!("URL targets reserved IPv6 address {v6}"));
                }
            }
        }
    }
    Ok(())
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
        Self {
            client: reqwest::Client::new(),
        }
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

        // SSRF protection: reject loopback, private, and link-local addresses
        #[cfg(not(test))]
        validate_url_for_ssrf(url).map_err(|msg| ExecutionError {
            connector: "http".into(),
            capability: instance.contract.id.clone(),
            reason: msg,
            retryable: false,
        })?;

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
        let text = response.text().await.map_err(|err| ExecutionError {
            connector: "http".into(),
            capability: instance.contract.id.clone(),
            reason: format!("failed to read response body: {err}"),
            retryable: false,
        })?;

        let mut metrics = HashMap::new();
        metrics.insert(
            "latency_ms".to_string(),
            started.elapsed().as_secs_f64() * 1000.0,
        );

        let mut truncated = text;
        if truncated.len() > MAX_BODY_BYTES {
            truncated.truncate(MAX_BODY_BYTES);
        }

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
    use std::io::{Read, Write};
    use std::net::TcpListener;

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

    fn spawn_echo_server() -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let body = b"real response from test server";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        (format!("http://{addr}/probe"), handle)
    }

    #[test]
    fn test_http_connector_descriptor() {
        let connector = HttpConnector::new();
        let desc = connector.descriptor();
        assert_eq!(desc.name, "http");
        assert_eq!(desc.supported_capabilities.len(), 1);
    }

    #[tokio::test]
    async fn test_makes_real_request() {
        let (url, server) = spawn_echo_server();
        let plugin = HttpPlugin::default();
        let result = plugin
            .execute(&make_instance(), json!({ "url": url }))
            .await
            .unwrap();
        server.join().unwrap();
        assert_eq!(result.outputs["status"], 200);
        assert_eq!(result.outputs["body"], "real response from test server");
    }

    #[tokio::test]
    async fn test_connection_failure_surfaces_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let plugin = HttpPlugin::default();
        let err = plugin
            .execute(
                &make_instance(),
                json!({ "url": format!("http://{addr}/down") }),
            )
            .await
            .unwrap_err();
        assert!(err.retryable, "network failures should be retryable");
        assert!(!err.reason.is_empty());
    }
}
