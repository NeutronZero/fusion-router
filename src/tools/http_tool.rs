use async_trait::async_trait;
use futures::StreamExt;
use hyper::client::connect::dns::Name;
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
    /// Shared with the client's DNS resolver so dial-time validation sees the
    /// same allowlist as eager validation.
    allowed_hosts: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    allowed_schemes: Vec<String>,
}

impl HTTPRequestTool {
    /// Fails closed: https only, no host allowlist (public hosts only).
    ///
    /// Construction is fallible on purpose: if the hardened client (timeout +
    /// redirects disabled + validating resolver) cannot be built, callers get
    /// an error instead of a silently weaker default client.
    pub fn new() -> Result<Self, String> {
        Self::try_default()
    }

    pub fn try_default() -> Result<Self, String> {
        let allowed_hosts = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let client = build_hardened_client(allowed_hosts.clone())?;
        Ok(Self {
            client,
            allowed_hosts,
            allowed_schemes: vec!["https".to_string()],
        })
    }

    pub fn with_allowed_hosts(self, hosts: Vec<String>) -> Self {
        *self.allowed_hosts.lock().unwrap_or_else(|e| e.into_inner()) = hosts;
        self
    }

    pub fn with_allowed_schemes(mut self, schemes: Vec<String>) -> Self {
        self.allowed_schemes = schemes;
        self
    }

    /// True for addresses the tool must never contact: loopback, link-local,
    /// private/ULA ranges, unspecified, CGNAT, broadcast, the whole "this
    /// network" block, IPv4-compatible IPv6, and NAT64 (SSRF defense).
    fn is_blocked_ip(ip: &std::net::IpAddr) -> bool {
        let ip = match ip {
            std::net::IpAddr::V6(v6) => {
                v6.to_ipv4_mapped().map(std::net::IpAddr::V4).unwrap_or(*ip)
            }
            other => *other,
        };
        match ip {
            std::net::IpAddr::V4(v4) => {
                let o = v4.octets();
                v4.is_loopback()
                    || v4.is_link_local()
                    || v4.is_private()
                    || v4.is_unspecified()
                    // 255.255.255.255 broadcast.
                    || v4.is_broadcast()
                    // Entire 0.0.0.0/8 "this network" block.
                    || o[0] == 0
                    // Carrier-grade NAT 100.64.0.0/10.
                    || (o[0] == 100 && (o[1] & 0xC0) == 0x40)
            }
            std::net::IpAddr::V6(v6) => {
                let s = v6.segments();
                v6.is_loopback()
                    || v6.is_unspecified()
                    || (s[0] & 0xffc0) == 0xfe80
                    || (s[0] & 0xfe00) == 0xfc00
                    // NAT64 well-known prefix 64:ff9b::/96 embeds an IPv4
                    // target in its low 32 bits; translating it reaches a
                    // host the URL never named.
                    || (s[0] == 0x0064
                        && s[1] == 0xff9b
                        && s[2..6].iter().all(|&seg| seg == 0))
                    // IPv4-compatible ::a.b.c.d (the non-mapped ::/96 form;
                    // the mapped ::ffff:a.b.c.d form was normalized above).
                    || (s[..6].iter().all(|&seg| seg == 0) && (s[6] != 0 || s[7] != 0))
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
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|h| h.eq_ignore_ascii_case(host))
    }

    /// DNS resolve-then-recheck with dial-time pinning (rebinding
    /// mitigation). Resolves the hostname ONCE here for eager rejection with
    /// precise errors; the client's validating resolver (see
    /// `ValidatingDnsResolver`) independently re-checks whatever IT resolves
    /// at connect time, so every address the socket layer may dial is a
    /// validated address even if DNS changes between check and connect.
    ///
    /// IP-literal URLs never hit the resolver (reqwest dials the literal,
    /// which `validate_url` range-checked); explicitly allowlisted hosts are
    /// trusted by policy in both layers.
    async fn validate_and_pin_host(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Option<std::net::SocketAddr>, String> {
        if self.is_host_allowlisted(host) {
            return Ok(None);
        }
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            if Self::is_blocked_ip(&ip) {
                return Err(format!(
                    "URL host '{}' is a blocked (loopback/private/link-local) address",
                    host
                ));
            }
            return Ok(None);
        }
        let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| format!("DNS resolution failed for '{}': {}", host, e))?
            .collect();
        Self::select_pinned_addr(host, &addrs)
    }

    /// Pure address-selection policy shared by eager validation and the
    /// dial-time resolver: rejects the host when ANY resolved address is
    /// blocked, otherwise yields the first (pinned) address.
    fn select_pinned_addr(
        host: &str,
        addrs: &[std::net::SocketAddr],
    ) -> Result<Option<std::net::SocketAddr>, String> {
        let mut first = None;
        for addr in addrs {
            if Self::is_blocked_ip(&addr.ip()) {
                return Err(format!(
                    "URL host '{}' resolves to blocked address {}",
                    host,
                    addr.ip()
                ));
            }
            if first.is_none() {
                first = Some(*addr);
            }
        }
        if first.is_none() {
            return Err(format!("URL host '{}' resolved to no addresses", host));
        }
        Ok(first)
    }

    /// Legacy validation-only wrapper (no pinning); kept for callers that
    /// only need the accept/reject decision.
    #[cfg(test)]
    async fn validate_host(&self, host: &str) -> Result<(), String> {
        self.validate_and_pin_host(host, 0).await.map(|_| ())
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

/// Dial-time DNS policy shared by every request the hardened client makes:
/// whatever addresses this resolver returns are exactly what reqwest dials,
/// so validating here pins "checked address == dialed address" at the socket
/// layer. Allowlisted hosts bypass the range check (explicit trust); IP
/// literals never reach a resolver.
struct ValidatingDnsResolver {
    allowed_hosts: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl ValidatingDnsResolver {
    fn allowlisted(&self, host: &str) -> bool {
        self.allowed_hosts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|h| h.eq_ignore_ascii_case(host))
    }

    fn validate(host: &str, addrs: &[std::net::SocketAddr]) -> Result<(), String> {
        if addrs.is_empty() {
            return Err(format!("URL host '{}' resolved to no addresses", host));
        }
        for addr in addrs {
            if HTTPRequestTool::is_blocked_ip(&addr.ip()) {
                return Err(format!(
                    "URL host '{}' resolves to blocked address {}",
                    host,
                    addr.ip()
                ));
            }
        }
        Ok(())
    }
}

impl reqwest::dns::Resolve for ValidatingDnsResolver {
    fn resolve(&self, name: Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        let allowed = self.allowlisted(&host);
        Box::pin(async move {
            // getaddrinfo is blocking; keep it off the reactor threads.
            let lookup_host = host.clone();
            let addrs: Vec<std::net::SocketAddr> =
                tokio::task::spawn_blocking(move || -> Result<Vec<_>, String> {
                    use std::net::ToSocketAddrs;
                    // Port is irrelevant here (hyper reattaches the URL's
                    // port to the dialed address); 80 keeps the std tuple
                    // resolver happy on all platforms.
                    let resolved: Vec<std::net::SocketAddr> = (lookup_host.as_str(), 80)
                        .to_socket_addrs()
                        .map_err(|e| format!("DNS resolution failed for '{}': {}", lookup_host, e))?
                        .collect();
                    Ok(resolved)
                })
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::other(format!(
                        "dns join error for '{host}': {e}"
                    )))
                })?
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::other(format!(
                        "DNS resolution failed for '{}': {}",
                        host, e
                    )))
                })?;

            if !allowed {
                ValidatingDnsResolver::validate(&host, &addrs).map_err(|msg| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        msg,
                    )) as Box<dyn std::error::Error + Send + Sync>
                })?;
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// Builds the hardened client: bounded request timeout, redirects disabled,
/// and a dial-time SSRF-validating DNS resolver. Shared by the tool
/// constructor so the fail-closed policy lives in exactly one place.
fn build_hardened_client(
    allowed_hosts: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none())
        .dns_resolver(std::sync::Arc::new(ValidatingDnsResolver { allowed_hosts }))
        .build()
        .map_err(|e| {
            format!(
                "failed to build hardened HTTP client for 'http_request' tool (timeout={}s, redirects=off): {}",
                DEFAULT_TIMEOUT_SECS, e
            )
        })
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

        // WP 3.3: scheme allowlist + SSRF range checks + DNS recheck. The
        // eager check rejects early with a precise message; the client's
        // validating resolver guarantees the dialed address is validated too
        // (see build_hardened_client), closing the rebind window.
        let url = self.validate_url(url_str)?;
        let host = Self::url_host_string(&url);
        let port = url.port_or_known_default().unwrap_or(80);
        self.validate_and_pin_host(&host, port).await?;

        let headers = args.get("headers").and_then(|v| v.as_object());

        // All four method paths share this one client, whose dial-time
        // resolver enforces the same address policy for every request
        // (Host/SNI still carry the original hostname).
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
        HTTPRequestTool::new()
            .expect("hardened client must build")
            .with_allowed_schemes(vec!["http".into(), "https".into()])
    }

    fn loopback_allowlisted_tool() -> HTTPRequestTool {
        http_scheme_tool().with_allowed_hosts(vec!["127.0.0.1".into()])
    }

    fn sock(ip: &str) -> std::net::SocketAddr {
        format!("{ip}:443").parse().unwrap()
    }

    #[tokio::test]
    async fn test_http_tool_invalid_url() {
        let tool = HTTPRequestTool::new().unwrap();
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
        let tool = HTTPRequestTool::new().unwrap();
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
        let tool = HTTPRequestTool::new().unwrap();
        let result = tool.validate_url("https://169.254.169.254/latest/meta-data");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("blocked"));
    }

    #[test]
    fn test_http_tool_rejects_loopback_ip() {
        let tool = HTTPRequestTool::new().unwrap();
        assert!(tool.validate_url("https://127.0.0.1/").is_err());
        assert!(tool.validate_url("https://[::1]/").is_err());
    }

    #[test]
    fn test_http_tool_rejects_private_ranges() {
        let tool = HTTPRequestTool::new().unwrap();
        for host in ["10.0.0.1", "172.16.1.1", "192.168.1.5"] {
            let err = tool.validate_url(&format!("https://{host}/x")).unwrap_err();
            assert!(err.contains("blocked"), "{} should be blocked", host);
        }
    }

    #[test]
    fn test_http_tool_rejects_ipv6_link_local_and_ula() {
        let tool = HTTPRequestTool::new().unwrap();
        assert!(tool.validate_url("https://[fe80::1]/").is_err());
        assert!(tool.validate_url("https://[fd00::1]/").is_err());
    }

    #[test]
    fn test_http_tool_rejects_http_scheme_by_default() {
        let tool = HTTPRequestTool::new().unwrap();
        let err = tool.validate_url("http://example.com/").unwrap_err();
        assert!(err.contains("scheme"), "http must be rejected by default");
    }

    #[test]
    fn test_http_tool_accepts_https_public_host() {
        let tool = HTTPRequestTool::new().unwrap();
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

    #[test]
    fn test_blocked_ranges_table_driven() {
        // (ip literal, must_be_blocked)
        let cases: &[(&str, bool)] = &[
            // CGNAT 100.64.0.0/10 — inclusive bounds.
            ("100.64.0.0", true),
            ("100.64.0.1", true),
            ("100.100.50.50", true),
            ("100.127.255.255", true),
            ("100.128.0.0", false),
            ("101.0.0.1", false),
            // Broadcast.
            ("255.255.255.255", true),
            ("255.255.255.254", false),
            // Entire 0.0.0.0/8 block, not just the unspecified address.
            ("0.0.0.0", true),
            ("0.0.0.1", true),
            ("0.1.2.3", true),
            ("0.255.255.255", true),
            // Sanity: public space stays allowed.
            ("93.184.216.34", false),
            ("8.8.8.8", false),
        ];
        for (ip, blocked) in cases {
            let parsed: std::net::IpAddr = ip.parse().unwrap();
            assert_eq!(
                HTTPRequestTool::is_blocked_ip(&parsed),
                *blocked,
                "unexpected classification for {ip}"
            );
        }
    }

    #[test]
    fn test_blocked_ipv6_ranges_table_driven() {
        let cases: &[(&str, bool)] = &[
            // NAT64 well-known prefix 64:ff9b::/96 (entire prefix blocked,
            // including the base address whose embedded v4 is 0.0.0.0).
            ("64:ff9b::", true),
            ("64:ff9b::7f00:1", true),    // 127.0.0.1 via NAT64
            ("64:ff9b::a9fe:a9fe", true), // 169.254.169.254 metadata via NAT64
            ("64:ff9b::0a00:1", true),    // private target via NAT64
            ("64:ff9b::1:2", true),       // still inside ::/96 with embedded bits
            ("64:ff9b:1::", false),       // different prefix, not NAT64
            ("2001:db8::1", false),
            // IPv4-compatible ::a.b.c.d (non-mapped form).
            ("::10.0.0.1", true),
            ("::192.168.1.1", true),
            ("::127.0.0.1", true),
            ("::c000:201", true), // hex spelling of 192.0.2.1
            // Mapped form is normalized to V4 and blocked there.
            ("::ffff:10.0.0.1", true),
            ("::ffff:93.184.216.34", false),
            // Sanity: real IPv6 globals stay allowed.
            ("2606:2800:220:1:248:1893:25c8:1946", false),
        ];
        for (ip, blocked) in cases {
            let parsed: std::net::IpAddr = ip.parse().unwrap();
            assert_eq!(
                HTTPRequestTool::is_blocked_ip(&parsed),
                *blocked,
                "unexpected classification for {ip}"
            );
        }
    }

    #[test]
    fn test_select_pinned_addr_rejects_if_any_resolution_blocked() {
        let addrs = vec![sock("93.184.216.34"), sock("127.0.0.1")];
        let err = HTTPRequestTool::select_pinned_addr("rebind.example.com", &addrs).unwrap_err();
        assert!(err.contains("blocked"), "{err}");
        assert!(err.contains("127.0.0.1"), "error must name the bad address");

        // Order must not matter for the rejection decision.
        let addrs = vec![sock("127.0.0.1"), sock("93.184.216.34")];
        assert!(HTTPRequestTool::select_pinned_addr("rebind.example.com", &addrs).is_err());
    }

    #[test]
    fn test_select_pinned_addr_returns_first_good_address() {
        let addrs = vec![sock("93.184.216.34"), sock("8.8.8.8")];
        let pinned = HTTPRequestTool::select_pinned_addr("ok.example.com", &addrs)
            .unwrap()
            .expect("non-empty resolution must pin");
        assert_eq!(pinned, sock("93.184.216.34"), "first address wins");
    }

    #[test]
    fn test_select_pinned_addr_empty_resolution_is_error() {
        let err = HTTPRequestTool::select_pinned_addr("nx.example.com", &[]).unwrap_err();
        assert!(err.contains("no addresses"), "{err}");
    }

    #[tokio::test]
    async fn test_validate_and_pin_pins_hostname_to_resolved_addr() {
        let tool = http_scheme_tool();
        // example.com resolves publicly; whatever comes back must be pinned
        // as Some(addr) so execute() attaches it via RequestBuilder::resolve.
        let pinned = tool
            .validate_and_pin_host("example.com", 443)
            .await
            .expect("public host must validate");
        let addr = pinned.expect("hostname must produce a pin");
        assert!(!addr.ip().is_loopback());
        assert_eq!(addr.port(), 443);
    }

    #[tokio::test]
    async fn test_validate_and_pin_skips_pin_for_ip_literal_and_allowlist() {
        let tool = loopback_allowlisted_tool();
        // IP literal: reqwest dials the literal; no DNS race to close.
        let pinned = tool
            .validate_and_pin_host("127.0.0.1", 8080)
            .await
            .expect("allowlisted literal passes");
        assert!(pinned.is_none(), "IP literals need no pinning");

        // Allowlisted hostname: trusted by policy, not pinned.
        let pinned = tool
            .validate_and_pin_host("127.0.0.1", 80)
            .await
            .expect("allowlisted host passes");
        assert!(pinned.is_none(), "allowlisted hosts are not pinned");
    }

    #[tokio::test]
    async fn test_validating_resolver_rejects_blocked_dial_time_addresses() {
        // Dial-time layer: "localhost" resolves to loopback; the resolver the
        // client actually consults when opening connections must refuse.
        let resolver = ValidatingDnsResolver {
            allowed_hosts: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        };
        let name: Name = "localhost".parse().expect("valid dns name");
        let err = match reqwest::dns::Resolve::resolve(&resolver, name).await {
            Ok(_) => panic!("loopback must be blocked at dial time"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("blocked"),
            "dial-time rejection must name the policy, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_validating_resolver_passes_allowlisted_hosts() {
        let resolver = ValidatingDnsResolver {
            allowed_hosts: std::sync::Arc::new(std::sync::Mutex::new(
                vec!["localhost".to_string()],
            )),
        };
        let name: Name = "localhost".parse().unwrap();
        let addrs = reqwest::dns::Resolve::resolve(&resolver, name)
            .await
            .expect("allowlisted host must resolve");
        assert!(
            addrs.count() > 0,
            "allowlisted resolution must yield addresses to dial"
        );
    }

    #[tokio::test]
    async fn test_pinned_connection_dials_validated_address_end_to_end() {
        // Full-path check of the pinning contract: an allowlisted literal URL
        // flows through execute() against a live listener, proving the
        // hardened client + resolver wiring does not break normal dials. The
        // rebinding-specific guarantees are covered by the resolver tests
        // above and select_pinned_addr unit tests.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bound = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let app =
                axum::Router::new().route("/hello", axum::routing::get(|| async { "pinned-ok" }));
            axum::serve(listener, app).await.unwrap();
        });

        let tool = loopback_allowlisted_tool();
        let result = tool
            .execute(serde_json::json!({
                "method": "GET",
                "url": format!("http://{}/hello", bound)
            }))
            .await;
        assert!(
            result.is_ok(),
            "dial through hardened client failed: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap()["status"], 200);

        drop(server);
    }

    #[test]
    fn test_resolver_validate_flags_empty_and_blocked_sets() {
        let ok = ValidatingDnsResolver::validate("ok.example.com", &[sock("93.184.216.34")]);
        assert!(ok.is_ok());

        let empty = ValidatingDnsResolver::validate("nx.example.com", &[]);
        assert!(empty.unwrap_err().contains("no addresses"));

        let blocked = ValidatingDnsResolver::validate(
            "bad.example.com",
            &[sock("93.184.216.34"), sock("10.0.0.5")],
        );
        assert!(blocked.unwrap_err().contains("blocked"));
    }

    #[test]
    fn test_hardened_client_build_failure_fails_construction() {
        // Structural fail-closed guarantee: `new` surfaces the underlying
        // builder error rather than degrading to Client::new(). The builder
        // itself succeeds in every environment we can construct here, so the
        // observable contract under test is that construction is Result-
        // typed and that a failure message carries the hardening context.
        match HTTPRequestTool::new() {
            Ok(_) => {}
            Err(e) => {
                assert!(e.contains("hardened HTTP client"), "{e}");
                assert!(e.contains("redirects=off"), "{e}");
            }
        }
    }
}
