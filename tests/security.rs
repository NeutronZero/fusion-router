use axum::{
    extract::Request,
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Router,
};

use fusion_router::config::AuthConfig;
use fusion_router::middleware::auth::auth_middleware;
use fusion_router::tools::builtin::FileReadTool;
use fusion_router::tools::ShellCommandTool;
use fusion_router::tools::Tool;

#[tokio::test]
async fn test_api_key_bruteforce() {
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
    for key in &["wrong-key", "another-key", "hack-key", "", "invalid"] {
        let res = client
            .get(format!("http://{}/", addr))
            .header("x-api-key", *key)
            .send()
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "key {:?} should be rejected",
            key
        );
    }
}

#[tokio::test]
async fn test_path_traversal() {
    let tmp = std::env::temp_dir();
    let tool = FileReadTool::new(tmp.to_string_lossy().to_string());
    let result = tool.execute(serde_json::json!({"path": "../../etc/passwd"})).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("Path traversal") || err.contains("not found") || err.contains("inaccessible")
    );
}

#[tokio::test]
async fn test_shell_injection() {
    let tool = ShellCommandTool::new(vec!["cmd".to_string(), "echo".to_string()], 5);

    let result = tool
        .execute(serde_json::json!({
            "command": "cmd /c rm -rf /"
        }))
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("not in allowed list"));

    let result2 = tool
        .execute(serde_json::json!({
            "command": "cmd",
            "args": ["/c", "echo", "hello"]
        }))
        .await;
    assert!(result2.is_ok());
}

#[tokio::test]
async fn test_oversized_payload() {
    async fn limit_body_size(
        req: Request,
        next: Next,
    ) -> Result<Response, (axum::http::StatusCode, String)> {
        const MAX_SIZE: usize = 1024;
        let content_length = req
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        if content_length > MAX_SIZE {
            return Err((
                axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                "Payload too large".to_string(),
            ));
        }
        Ok(next.run(req).await)
    }

    async fn handler() -> &'static str {
        "ok"
    }

    let app = Router::new()
        .route("/", post(handler))
        .layer(middleware::from_fn(limit_body_size));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let large_body = "x".repeat(2048);
    let res = client
        .post(format!("http://{}/", addr))
        .body(large_body)
        .header("content-type", "text/plain")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
}
