use axum::{
    extract::Request,
    http::HeaderValue,
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

const REQUEST_ID_HEADER: &str = "x-request-id";

pub async fn request_id_middleware(
    mut req: Request,
    next: Next,
) -> Response {
    let request_id = req
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    req.extensions_mut().insert(request_id.clone());

    let mut res = next.run(req).await;

    if let Ok(val) = HeaderValue::from_str(&request_id) {
        res.headers_mut().insert(REQUEST_ID_HEADER, val);
    }

    tracing::info!(request_id = %request_id, "request completed");

    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use reqwest::StatusCode;

    #[tokio::test]
    async fn test_request_id_generated() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(request_id_middleware));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let res = client.get(format!("http://{}/", addr)).send().await.unwrap();
        assert!(res.headers().contains_key("x-request-id"));
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_request_id_passthrough() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(request_id_middleware));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let res = client
            .get(format!("http://{}/", addr))
            .header("x-request-id", "client-provided-id")
            .send()
            .await
            .unwrap();
        assert_eq!(
            res.headers().get("x-request-id").unwrap(),
            "client-provided-id"
        );
        assert_eq!(res.status(), StatusCode::OK);
    }
}
