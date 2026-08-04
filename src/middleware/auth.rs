use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::config::AuthConfig;

/// Authenticated client identity for downstream middleware (e.g. the rate
/// limiter). Carries the SHA-256 of the presented API key — never the raw key.
#[derive(Debug, Clone)]
pub struct ClientIdentity(pub String);

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

pub async fn auth_middleware(
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let Some(auth_config) = req.extensions().get::<AuthConfig>().cloned() else {
        // Fail closed: a router wiring this middleware without providing the
        // AuthConfig extension must not silently become unauthenticated.
        tracing::warn!(
            path = %req.uri().path(),
            "auth middleware invoked without AuthConfig extension, rejecting request"
        );
        return Err((
            StatusCode::UNAUTHORIZED,
            json!({"error": "unauthorized"}).to_string(),
        ));
    };

    if !auth_config.enabled {
        return Ok(next.run(req).await);
    }

    let path = req.uri().path();
    let whitelisted = path == "/health" || path == "/ready" || path == "/metrics";
    if whitelisted {
        return Ok(next.run(req).await);
    }

    let api_key = req
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    match api_key {
        Some(key) if api_key_matches(&key, &auth_config.api_keys) => {
            let mut req = req;
            req.extensions_mut()
                .insert(ClientIdentity(key_identity(&key)));
            Ok(next.run(req).await)
        }
        _ => Err((
            StatusCode::UNAUTHORIZED,
            json!({"error": "unauthorized"}).to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};

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
        assert!(!api_key_matches(&oversized, &[oversized.clone()]));
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

    #[tokio::test]
    async fn test_auth_disabled_passthrough() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(auth_middleware))
            .layer(axum::Extension(AuthConfig { enabled: false, api_keys: vec![] }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let res = client.get(format!("http://{}/", addr)).send().await.unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_valid_key_passes() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(auth_middleware))
            .layer(axum::Extension(AuthConfig {
                enabled: true,
                api_keys: vec!["valid-key".into()],
            }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let res = client
            .get(format!("http://{}/", addr))
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
            .layer(axum::Extension(AuthConfig {
                enabled: true,
                api_keys: vec!["valid-key".into()],
            }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let res = client
            .get(format!("http://{}/", addr))
            .header("x-api-key", "wrong-key")
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_whitelisted_paths_skip_auth() {
        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(auth_middleware))
            .layer(axum::Extension(AuthConfig {
                enabled: true,
                api_keys: vec!["valid-key".into()],
            }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let res = client.get(format!("http://{}/health", addr)).send().await.unwrap();
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
        let res = client.get(format!("http://{}/", addr)).send().await.unwrap();
        assert_eq!(
            res.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "missing AuthConfig extension must fail closed, not open"
        );
    }
}
