use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::config::AuthConfig;

pub async fn auth_middleware(
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let auth_config = req
        .extensions()
        .get::<AuthConfig>()
        .cloned()
        .unwrap_or_default();

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
        Some(key) if auth_config.api_keys.contains(&key) => Ok(next.run(req).await),
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
}
