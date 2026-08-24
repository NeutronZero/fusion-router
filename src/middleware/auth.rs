use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::config::{error::ReloadError, manager::{ConfigSnapshot, ConfigSubscriber}, AuthConfig};

/// Authenticated client identity for downstream middleware (e.g. the rate
/// limiter). Carries the SHA-256 of the presented API key — never the raw key.
#[derive(Debug, Clone)]
pub struct ClientIdentity(pub String);

/// What a key is allowed to do. Every valid key may use the chat surface;
/// operator-gated surfaces (operations API, executions API, /metrics) require
/// an explicit `operator` grant.
#[derive(Debug, Clone)]
pub struct KeyGrants {
    pub operator: bool,
}

/// Immutable runtime view of the auth configuration, keyed by SHA-256 digest
/// of each raw API key so lookups never hold raw key material.
#[derive(Debug, Clone)]
pub struct AuthSnapshot {
    pub enabled: bool,
    pub keys: HashMap<String, KeyGrants>,
}

/// Live handle to the current [`AuthSnapshot`]. Shared as an axum extension
/// and swapped atomically by the auth reload subscriber, so rotated keys take
/// effect on the next request without a restart.
#[derive(Clone)]
pub struct AuthHandle(pub Arc<ArcSwap<AuthSnapshot>>);

impl AuthHandle {
    /// Builds a snapshot from configured keys.
    ///
    /// Key syntax (backward compatible):
    /// - `"sk-abc"`          → chat-only
    /// - `"sk-abc:chat"`     → chat-only (explicit)
    /// - `"sk-abc:operator"` → operator grants (includes chat surfaces)
    pub fn from_config(config: &AuthConfig) -> Self {
        let mut keys = HashMap::new();
        for entry in &config.api_keys {
            let Some((raw, scope)) = parse_key_entry(entry) else {
                tracing::warn!("ignoring blank api_keys entry");
                continue;
            };
            if raw.len() > MAX_API_KEY_BYTES {
                tracing::warn!("ignoring oversized api_keys entry");
                continue;
            }
            let digest = hex_encode(&Sha256::digest(raw.as_bytes()));
            let operator = matches!(scope, KeyScope::Operator);
            keys.insert(digest, KeyGrants { operator });
        }
        Self(Arc::new(ArcSwap::from_pointee(AuthSnapshot {
            enabled: config.enabled,
            keys,
        })))
    }

    pub fn load(&self) -> Arc<AuthSnapshot> {
        self.0.load_full()
    }

    /// Atomically swaps to a new configuration (used by the reload subscriber).
    pub fn swap_to(&self, config: &AuthConfig) {
        let fresh = Self::from_config(config);
        self.0.store(fresh.0.load_full());
    }
}

enum KeyScope {
    Chat,
    Operator,
}

fn parse_key_entry(entry: &str) -> Option<(String, KeyScope)> {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.rsplit_once(':') {
        Some((raw, "operator")) => Some((raw.trim().to_string(), KeyScope::Operator)),
        Some((raw, "chat")) => Some((raw.trim().to_string(), KeyScope::Chat)),
        // A colon with any other suffix would silently change meaning; treat
        // unknown scopes as invalid rather than part of the key (fail closed).
        Some(_) => None,
        None => Some((trimmed.to_string(), KeyScope::Chat)),
    }
}

/// Maximum accepted API key length in bytes. Rejecting oversized keys before
/// hashing prevents digest work amplification and memory abuse.
pub const MAX_API_KEY_BYTES: usize = 1024;

/// Constant-time API key comparison (M3 / ADR-035): both sides are hashed
/// with SHA-256 and compared with `subtle::ConstantTimeEq`, so timing does
/// not reveal key length, prefix, or position in the configured list.
pub fn api_key_matches(provided: &str, configured: &[String]) -> bool {
    if provided.len() > MAX_API_KEY_BYTES || provided.is_empty() {
        return false;
    }
    let provided_digest = Sha256::digest(provided.as_bytes());
    configured.iter().any(|candidate| {
        candidate.len() <= MAX_API_KEY_BYTES
            && !candidate.is_empty()
            && Sha256::digest(candidate.as_bytes())
                .ct_eq(&provided_digest)
                .into()
    })
}

fn key_identity(provided: &str) -> String {
    format!("key:{}", hex_encode(&Sha256::digest(provided.as_bytes())))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Surfaces gated behind the operator grant. Chat endpoints remain available
/// to every valid key.
fn requires_operator(path: &str) -> bool {
    path == "/metrics"
        || path.starts_with("/v1/operations")
        || path.starts_with("/v1/executions")
}

fn unauthorized() -> (StatusCode, String) {
    (StatusCode::UNAUTHORIZED, json!({"error": "unauthorized"}).to_string())
}

fn forbidden() -> (StatusCode, String) {
    (
        StatusCode::FORBIDDEN,
        json!({"error": "forbidden", "reason": "operator scope required"}).to_string(),
    )
}

pub async fn auth_middleware(
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let Some(handle) = req.extensions().get::<AuthHandle>().cloned() else {
        // Fail closed: a router wiring this middleware without providing the
        // AuthHandle extension must not silently become unauthenticated.
        tracing::warn!(
            path = %req.uri().path(),
            "auth middleware invoked without AuthHandle extension, rejecting request"
        );
        return Err(unauthorized());
    };

    let snapshot = handle.load();
    if !snapshot.enabled {
        return Ok(next.run(req).await);
    }

    let path = req.uri().path();
    if path == "/health" || path == "/ready" {
        return Ok(next.run(req).await);
    }

    let api_key = req
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let Some(key) = api_key else {
        return Err(unauthorized());
    };
    if key.is_empty() || key.len() > MAX_API_KEY_BYTES {
        return Err(unauthorized());
    }

    let digest = hex_encode(&Sha256::digest(key.as_bytes()));
    let Some(grants) = snapshot.keys.get(&digest) else {
        return Err(unauthorized());
    };

    if requires_operator(path) && !grants.operator {
        tracing::warn!(path = %path, "valid key attempted operator-surface access without operator scope");
        return Err(forbidden());
    }

    let mut req = req;
    req.extensions_mut()
        .insert(ClientIdentity(key_identity(&key)));
    req.extensions_mut().insert(grants.clone());
    Ok(next.run(req).await)
}

/// Reload subscriber keeping [`AuthHandle`] in sync with config reloads.
///
/// prepare() parses and stages the new snapshot (a malformed entry fails the
/// whole reload); commit() swaps it in atomically. Key rotation therefore
/// applies to the next request after SIGHUP — no restart.
pub struct AuthReloader {
    pub handle: AuthHandle,
    staged: std::sync::Mutex<Option<AuthSnapshot>>,
}

impl AuthReloader {
    pub fn new(handle: AuthHandle) -> Self {
        Self { handle, staged: std::sync::Mutex::new(None) }
    }
}

impl ConfigSubscriber for AuthReloader {
    fn priority(&self) -> u8 {
        1
    }

    fn prepare(&self, _old: &ConfigSnapshot, new: &ConfigSnapshot) -> Result<(), ReloadError> {
        let candidate = AuthHandle::from_config(&new.config.auth);
        let snapshot = candidate.load();
        if snapshot.enabled && snapshot.keys.is_empty() {
            return Err(ReloadError::Subscriber {
                name: "auth".into(),
                reason: "auth enabled but no valid api_keys entries".into(),
            });
        }
        *self.staged.lock().unwrap_or_else(|e| e.into_inner()) = Some(
            AuthSnapshot {
                enabled: snapshot.enabled,
                keys: snapshot.keys.clone(),
            },
        );
        Ok(())
    }

    fn commit(&self, _generation: u64) {
        if let Some(snap) = self.staged.lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.handle.0.store(Arc::new(snap));
            tracing::info!("auth configuration applied (keys/scopes rotated)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};

    fn handle(keys: &[&str], enabled: bool) -> AuthHandle {
        AuthHandle::from_config(&AuthConfig {
            enabled,
            api_keys: keys.iter().map(|s| s.to_string()).collect(),
        })
    }

    #[test]
    fn test_api_key_matches_correct_key() {
        assert!(api_key_matches("sk-valid", &["sk-valid".into()]));
    }

    #[test]
    fn test_api_key_matches_any_configured_key() {
        assert!(api_key_matches("sk-b", &["sk-a".into(), "sk-b".into(), "sk-c".into()]));
    }

    #[test]
    fn test_api_key_matches_rejects_wrong_key() {
        assert!(!api_key_matches("sk-wrong", &["sk-valid".into()]));
    }

    #[test]
    fn test_api_key_matches_rejects_empty_key() {
        assert!(!api_key_matches("", &["sk-valid".into()]));
        assert!(!api_key_matches("sk-valid", &[String::new()]));
    }

    #[test]
    fn test_api_key_matches_rejects_oversize_key() {
        let oversized = "x".repeat(MAX_API_KEY_BYTES + 1);
        assert!(!api_key_matches(&oversized, std::slice::from_ref(&oversized)));
        let oversized_configured = "y".repeat(MAX_API_KEY_BYTES + 1);
        assert!(!api_key_matches("y", &[oversized_configured]));
    }

    #[test]
    fn test_api_key_matches_length_similar_keys_do_not_cross_match() {
        assert!(!api_key_matches("sk-a", &["sk-aa".into()]));
    }

    #[test]
    fn test_client_identity_is_hashed() {
        let id = key_identity("sk-valid");
        assert!(id.starts_with("key:"), "identity must be prefixed: {id}");
        assert!(!id.contains("sk-valid"), "identity must not leak the raw key: {id}");
        assert_eq!(id.len(), 4 + 64, "identity must be a SHA-256 hex digest");
        assert_eq!(key_identity("sk-valid"), key_identity("sk-valid"));
        assert_ne!(key_identity("sk-valid"), key_identity("sk-other"));
    }

    #[test]
    fn plain_key_grants_chat_only() {
        let h = handle(&["sk-plain"], true);
        let snap = h.load();
        let g = snap.keys.values().next().unwrap();
        assert!(!g.operator);
    }

    #[test]
    fn operator_suffix_grants_operator() {
        let h = handle(&["sk-op:operator"], true);
        let snap = h.load();
        let g = snap.keys.values().next().unwrap();
        assert!(g.operator);
    }

    #[test]
    fn unknown_scope_entry_is_rejected() {
        let h = handle(&["sk-x:bogus"], true);
        assert!(h.load().keys.is_empty(), "fail closed on unparseable scope");
    }

    #[tokio::test]
    async fn test_auth_disabled_passthrough() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(auth_middleware))
            .layer(axum::Extension(handle(&[], false)));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let res = client.get(format!("http://{addr}/")).send().await.unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_valid_key_passes() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(auth_middleware))
            .layer(axum::Extension(handle(&["valid-key"], true)));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let res = client
            .get(format!("http://{addr}/"))
            .header("x-api-key", "valid-key")
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_invalid_key_returns_401() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(auth_middleware))
            .layer(axum::Extension(handle(&["valid-key"], true)));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let res = client
            .get(format!("http://{addr}/"))
            .header("x-api-key", "wrong-key")
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_health_whitelisted_but_metrics_is_not() {
        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            .route("/metrics", get(|| async { "metrics" }))
            .layer(axum::middleware::from_fn(auth_middleware))
            .layer(axum::Extension(handle(&["valid-key"], true)));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let res = client.get(format!("http://{addr}/health")).send().await.unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::OK);

        let res = client.get(format!("http://{addr}/metrics")).send().await.unwrap();
        assert_eq!(
            res.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "/metrics must not be anonymously scrapeable"
        );
    }

    #[tokio::test]
    async fn test_chat_key_cannot_access_operator_surface() {
        let app = Router::new()
            .route("/v1/operations/dashboard", get(|| async { "ops" }))
            .route("/metrics", get(|| async { "metrics" }))
            .layer(axum::middleware::from_fn(auth_middleware))
            .layer(axum::Extension(handle(&["sk-chat"], true)));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        for path in ["/v1/operations/dashboard", "/metrics"] {
            let res = client
                .get(format!("http://{addr}{path}"))
                .header("x-api-key", "sk-chat")
                .send()
                .await
                .unwrap();
            assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN, "{path}");
        }
    }

    #[tokio::test]
    async fn test_operator_key_accesses_operator_surface() {
        let app = Router::new()
            .route("/metrics", get(|| async { "metrics" }))
            .layer(axum::middleware::from_fn(auth_middleware))
            .layer(axum::Extension(handle(&["sk-op:operator"], true)));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let res = client
            .get(format!("http://{addr}/metrics"))
            .header("x-api-key", "sk-op")
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_live_key_rotation_without_restart() {
        let shared = handle(&["old-key"], true);
        let ext = shared.clone();
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(auth_middleware))
            .layer(axum::Extension(ext));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();

        let res = client
            .get(format!("http://{addr}/"))
            .header("x-api-key", "old-key")
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::OK);

        // Rotate exactly like AuthReloader::commit does.
        shared.swap_to(&AuthConfig {
            enabled: true,
            api_keys: vec!["new-key".into()],
        });

        let res = client
            .get(format!("http://{addr}/"))
            .header("x-api-key", "old-key")
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED, "rotated-out key must die immediately");

        let res = client
            .get(format!("http://{addr}/"))
            .header("x-api-key", "new-key")
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_missing_config_fails_closed() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(auth_middleware));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let res = client.get(format!("http://{addr}/")).send().await.unwrap();
        assert_eq!(
            res.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "missing AuthHandle extension must fail closed, not open"
        );
    }
}
