use std::sync::Arc;

use axum::{routing::post, Router};
use tower_http::trace::TraceLayer;

use fusion_router::providers::ChatProvider;
use fusion_router::types::ChatCompletionRequest;

struct MockProvider;

#[async_trait::async_trait]
impl ChatProvider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<fusion_router::types::ChatCompletionResponse> {
        Ok(fusion_router::types::ChatCompletionResponse {
            id: "mock-id".to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: request.model.clone(),
            choices: vec![fusion_router::types::Choice {
                index: 0,
                message: fusion_router::types::ChatMessage {
                    role: "assistant".to_string(),
                    content: "Hello from mock!".to_string(),
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Some(fusion_router::types::Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        })
    }
}

#[tokio::test]
async fn test_chat_completion_endpoint() {
    let provider = Arc::new(MockProvider);
    let state = fusion_router::server::handlers::AppState { provider };

    let app = Router::new()
        .route("/v1/chat/completions", post(fusion_router::server::handlers::chat_completions))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/chat/completions", addr))
        .json(&serde_json::json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert!(body["choices"][0]["message"]["content"].as_str().unwrap().contains("Hello from mock"));
}
