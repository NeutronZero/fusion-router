use async_trait::async_trait;
use futures::StreamExt;
use serde_json::Value;

use super::Tool;

const MAX_BODY_BYTES: usize = 1_048_576;
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Headers a caller may never override on a tool request — the tool
/// establishes its own trust context, so spoofing identity to the target
/// (or pinning routing via Host) is blocked.
const BLOCKED_HEADERS: &[&str] = &["authorization", "host"];

pub struct HTTPRequestTool {
    client: reqwest::Client,
    allowed_hosts: Vec<String>,
    allowed_schemes: Vec<String>,
}

impl HTTPRequestTool {
    /// Fails closed: https only, no host allowlist (public hosts only).
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_allowed_hosts(mut self, hosts: Vec<String>) -> Self {
        self.allowed_hosts = hosts;
        self
    }

    pub fn with_allowed_schemes(mut self, schemes: Vec<String>) -> Self {
        self.allowed_schemes = schemes;
        self
    }

    /// True for addresses the tool must never contact: loopback, link-local,
    /// private/ULA ranges, and unspecified (SSRF defense, finding H1).
    fn is_blocked_ip(ip: &std::net::IpAddr) -> bool {
        let ip = match ip {
            std::net::IpAddr::V6(v6) => {
                v6.to_ipv4_mapped().map(std::net::IpAddr::V4).unwrap_or(*ip)
            }
            other => *other,
        };
        match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_link_local() || v4.is_private() || v4.is_unspecified()
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
            }
        }
    }

    /// Synchronous URL policy: parse, scheme allowlist, IP-literal range check.
    fn validate_url(&self, url_str: &str) -> Result<reqwest::Url, String> {
        let url = reqwest::Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))?;
        let scheme = url.scheme().to_ascii_lowercase();
        if !self
            .allowed_schemes
            .iter()
            .any(|s| s.eq_ignore_ascii_case(&scheme))
        {
            return Err(format!(
                "URL scheme '{}' is not allowed (allowed: {:?})",
                scheme, self.allowed_schemes
            ));
        }
        let host = url
            .host()
            .ok_or_else(|| "URL must have a host".to_string())?;
        match host {
            url::Host::Ipv4(v4) => {
                if !self.is_host_allowlisted(&v4.to_string())
                    && Self::is_blocked_ip(&std::net::IpAddr::V4(v4))
                {
                    return Err(format!(
                        "URL host '{}' resolves to a blocked (loopback/private/link-local) address",
                        v4
                    ));
                }
            }
            url::Host::Ipv6(v6) => {
                if !self.is_host_allowlisted(&v6.to_string())
                    && Self::is_blocked_ip(&std::net::IpAddr::V6(v6))
                {
                    return Err(format!(
                        "URL host '{}' resolves to a blocked (loopback/private/link-local) address",
                        v6
                    ));
                }
            }
            url::Host::Domain(_) => {}
        }
        Ok(url)
    }

    /// Bracket-free host string for DNS rechecks and allowlist matching
    /// (`host_str()` returns IPv6 literals with brackets).
    fn url_host_string(url: &reqwest::Url) -> String {
        match url.host() {
            Some(url::Host::Domain(d)) => d.to_string(),
            Some(url::Host::Ipv4(v)) => v.to_string(),
            Some(url::Host::Ipv6(v)) => v.to_string(),
            None => String::new(),
        }
    }

    fn is_host_allowlisted(&self, host: &str) -> bool {
        self.allowed_hosts
            .iter()
            .any(|h| h.eq_ignore_ascii_case(host))
    }

    /// DNS resolve-then-recheck (rebinding mitigation): every address a
    /// hostname resolves to must be non-blocked unless the host is
    /// explicitly allowlisted.
    async fn validate_host(&self, host: &str) -> Result<(), String> {
        if self.is_host_allowlisted(host) {
            return Ok(());
        }
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            if Self::is_blocked_ip(&ip) {
                return Err(format!(
                    "URL host '{}' is a blocked (loopback/private/link-local) address",
                    host
                ));
            }
            return Ok(());
        }
        let addrs = tokio::net::lookup_host((host, 0))
            .await
            .map_err(|e| format!("DNS resolution failed for '{}': {}", host, e))?;
        let mut any_addr = false;
        for addr in addrs {
            any_addr = true;
            if Self::is_blocked_ip(&addr.ip()) {
                return Err(format!(
                    "URL host '{}' resolves to blocked address {}",
                    host,
                    addr.ip()
                ));
            }
        }
        if !any_addr {
            return Err(format!("URL host '{}' resolved to no addresses", host));
        }
        Ok(())
    }

    async fn read_body_limited(
        &self,
        response: reqwest::Response,
    ) -> Result<(String, bool), String> {
        let mut body = Vec::new();
        let mut truncated = false;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("HTTP body read error: {}", e))?;
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                let remaining = MAX_BODY_BYTES.saturating_sub(body.len());
                body.extend_from_slice(&chunk[..remaining]);
                truncated = true;
                break;
            }
            body.extend_from_slice(&chunk);
        }
        Ok((String::from_utf8_lossy(&body).to_string(), truncated))
    }
}

impl Default for HTTPRequestTool {
    fn default() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Failed to build configured HTTP client for HTTPRequestTool, falling back to default Client");
                reqwest::Client::new()
            });
        Self {
            client,
            allowed_hosts: Vec::new(),
            allowed_schemes: vec!["https".to_string()],
        }
    }
}

#[async_trait]
impl Tool for HTTPRequestTool {
    fn name(&self) -> &str {
        "http_request"
    }

    fn description(&self) -> &str {
        "Makes HTTPS requests to external URLs. Supports GET, POST, PUT, DELETE methods."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "DELETE"],
                    "description": "HTTP method"
                },
                "url": {
                    "type": "string",
                    "description": "Request URL"
                },
                "headers": {
                    "type": "object",
                    "description": "Optional request headers (Authorization/Host overrides are ignored)"
                },
                "body": {
                    "type": "object",
                    "description": "Optional request body (for POST/PUT)"
                }
            },
            "required": ["method", "url"]
        })
    }

    async fn execute(&self, args: Value) -> Result<Value, String> {
        let method = args
            .get("method")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'method' argument".to_string())?;

        let url_str = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing 'url' argument".to_string())?;

        // WP 3.3: scheme allowlist + SSRF range checks + DNS recheck.
        let url = self.validate_url(url_str)?;
        let host = Self::url_host_string(&url);
        self.validate_host(&host).await?;

        let headers = args.get("headers").and_then(|v| v.as_object());

        let mut request = match method {
            "GET" => self.client.get(url.as_str()),
            "POST" => {
                let body = args.get("body").cloned().unwrap_or(Value::Null);
                self.client.post(url.as_str()).json(&body)
            }
            "PUT" => {
                let body = args.get("body").cloned().unwrap_or(Value::Null);
                self.client.put(url.as_str()).json(&body)
            }
            "DELETE" => self.client.delete(url.as_str()),
            _ => return Err(format!("Unsupported HTTP method: {}", method)),
        };

        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                if let Some(val_str) = value.as_str() {
                    let lower = key.to_ascii_lowercase();
                    if BLOCKED_HEADERS.contains(&lower.as_str()) {
                        continue;
                    }
                    request = request.header(key.as_str(), val_str);
                }
            }
        }

        let response = request
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        let status = response.status().as_u16();
        let (body, truncated) = self.read_body_limited(response).await?;

        Ok(serde_json::json!({
            "status": status,
            "body": body,
            "truncated": truncated,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http_scheme_tool() -> HTTPRequestTool {
        HTTPRequestTool::new().with_allowed_schemes(vec!["http".into(), "https".into()])
    }

    fn loopback_allowlisted_tool() -> HTTPRequestTool {
        http_scheme_tool().with_allowed_hosts(vec!["127.0.0.1".into()])
    }

    #[tokio::test]
    async fn test_http_tool_invalid_url() {
        let tool = HTTPRequestTool::new();
        let result = tool
            .execute(serde_json::json!({
                "method": "GET",
                "url": "not-a-valid-url"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_http_tool_missing_args() {
        let tool = HTTPRequestTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("'method'"));
    }

    #[tokio::test]
    async fn test_http_tool_unsupported_method() {
        let tool = loopback_allowlisted_tool();
        let result = tool
            .execute(serde_json::json!({
                "method": "PATCH",
                "url": "http://127.0.0.1:8080"
            }))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported"));
    }

    #[test]
    fn test_http_tool_rejects_metadata_ip() {
        let tool = HTTPRequestTool::new();
        let result = tool.validate_url("https://169.254.169.254/latest/meta-data");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("blocked"));
    }

    #[test]
    fn test_http_tool_rejects_loopback_ip() {
        let tool = HTTPRequestTool::new();
        assert!(tool.validate_url("https://127.0.0.1/").is_err());
        assert!(tool.validate_url("https://[::1]/").is_err());
    }

    #[test]
    fn test_http_tool_rejects_private_ranges() {
        let tool = HTTPRequestTool::new();
        for host in ["10.0.0.1", "172.16.1.1", "192.168.1.5"] {
            let err = tool.validate_url(&format!("https://{host}/x")).unwrap_err();
            assert!(err.contains("blocked"), "{} should be blocked", host);
        }
    }

    #[test]
    fn test_http_tool_rejects_ipv6_link_local_and_ula() {
        let tool = HTTPRequestTool::new();
        assert!(tool.validate_url("https://[fe80::1]/").is_err());
        assert!(tool.validate_url("https://[fd00::1]/").is_err());
    }

    #[test]
    fn test_http_tool_rejects_http_scheme_by_default() {
        let tool = HTTPRequestTool::new();
        let err = tool.validate_url("http://example.com/").unwrap_err();
        assert!(err.contains("scheme"), "http must be rejected by default");
    }

    #[test]
    fn test_http_tool_accepts_https_public_host() {
        let tool = HTTPRequestTool::new();
        assert!(tool.validate_url("https://example.com/").is_ok());
    }

    #[test]
    fn test_http_tool_allowlisted_host_bypasses_range_check() {
        let tool = loopback_allowlisted_tool();
        assert!(tool.validate_url("http://127.0.0.1:8080/").is_ok());
    }

    #[tokio::test]
    async fn test_http_tool_hostname_resolving_to_loopback_rejected() {
        let tool = http_scheme_tool();
        // "localhost" resolves to 127.0.0.1/::1 locally; the DNS recheck
        // must reject it even though the literal URL parse succeeded.
        let err = tool.validate_host("localhost").await.unwrap_err();
        assert!(
            err.contains("blocked"),
            "localhost must be blocked, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_http_tool_allowlisted_host_works() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let app = axum::Router::new()
                .route("/hello", axum::routing::get(|| async { "hello from tool" }));
            axum::serve(listener, app).await.unwrap();
        });

        let tool = loopback_allowlisted_tool();
        let result = tool
            .execute(serde_json::json!({
                "method": "GET",
                "url": format!("http://{}/hello", addr)
            }))
            .await;
        assert!(
            result.is_ok(),
            "allowlisted host should work: {:?}",
            result.err()
        );
        let val = result.unwrap();
        assert_eq!(val["status"], 200);
        assert!(val["body"]
            .as_str()
            .unwrap_or("")
            .contains("hello from tool"));
        assert_eq!(val["truncated"], false);
    }

    #[tokio::test]
    async fn test_http_tool_rejects_redirect_to_internal() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let app = axum::Router::new().route(
                "/redirect",
                axum::routing::get(|| async {
                    (
                        axum::http::StatusCode::FOUND,
                        [("Location", "http://127.0.0.1:1/secret")],
                        "redirecting",
                    )
                }),
            );
            axum::serve(listener, app).await.unwrap();
        });

        let tool = loopback_allowlisted_tool();
        let result = tool
            .execute(serde_json::json!({
                "method": "GET",
                "url": format!("http://{}/redirect", addr)
            }))
            .await;
        assert!(
            result.is_ok(),
            "redirect response must be returned, not followed"
        );
        let val = result.unwrap();
        assert_eq!(val["status"], 302, "redirect must not be followed");
        assert!(
            !val["body"].as_str().unwrap_or("").contains("secret"),
            "redirect target must never be fetched"
        );
    }

    #[tokio::test]
    async fn test_http_tool_truncates_oversized_body() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let payload = "x".repeat(3 * MAX_BODY_BYTES);
        tokio::spawn(async move {
            let app = axum::Router::new().route(
                "/big",
                axum::routing::get(move || {
                    let payload = payload.clone();
                    async move { payload }
                }),
            );
            axum::serve(listener, app).await.unwrap();
        });

        let tool = loopback_allowlisted_tool();
        let result = tool
            .execute(serde_json::json!({
                "method": "GET",
                "url": format!("http://{}/big", addr)
            }))
            .await;
        assert!(
            result.is_ok(),
            "oversized body must be capped, not failed: {:?}",
            result.err()
        );
        let val = result.unwrap();
        assert_eq!(val["status"], 200);
        assert_eq!(val["truncated"], true, "body must be flagged truncated");
        assert!(val["body"].as_str().unwrap_or("").len() <= MAX_BODY_BYTES);
    }

    #[tokio::test]
    async fn test_http_tool_drops_authorization_and_host_headers() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            use axum::http::HeaderMap;
            let app = axum::Router::new().route(
                "/echo",
                axum::routing::get(|headers: HeaderMap| async move {
                    axum::Json(serde_json::json!({
                        "auth": headers.get("authorization").map(|v| v.to_str().unwrap_or("").to_string()),
                        "host_header": headers.get("host").map(|v| v.to_str().unwrap_or("").to_string()),
                    }))
                }),
            );
            axum::serve(listener, app).await.unwrap();
        });

        let tool = loopback_allowlisted_tool();
        let result = tool
            .execute(serde_json::json!({
                "method": "GET",
                "url": format!("http://{}/echo", addr),
                "headers": {
                    "Authorization": "Bearer leaked-secret",
                    "Host": "evil.example.com",
                    "X-Custom": "kept"
                }
            }))
            .await;
        assert!(result.is_ok(), "request should succeed: {:?}", result.err());
        let val = result.unwrap();
        let body: Value =
            serde_json::from_str(val["body"].as_str().unwrap_or("")).unwrap_or(Value::Null);
        assert_eq!(
            body["auth"],
            Value::Null,
            "Authorization override must be dropped"
        );
        assert_ne!(
            body["host_header"].as_str().unwrap_or(""),
            "evil.example.com",
            "Host override must be dropped"
        );
    }
}
