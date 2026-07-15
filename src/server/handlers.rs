use axum::{extract::State, Json};
use std::sync::Arc;
use uuid::Uuid;

use crate::providers::Provider;
use crate::types::ChatCompletionRequest;

#[derive(Clone)]
pub struct AppState {
    pub provider: Arc<dyn Provider + Send + Sync>,
}

pub async fn chat_completions(
    State(state): State<AppState>,
    Json(request): Json<ChatCompletionRequest>,
) -> Json<crate::types::ChatCompletionResponse> {
    let request_id = Uuid::new_v4();
    let _span = tracing::info_span!("chat_completions", request_id = %request_id, model = %request.model);

    tracing::info!("received chat completion request");

    let response = state.provider.chat_completion(&request).await.unwrap_or_else(|e| {
        tracing::error!(error = %e, "provider call failed");
        crate::types::ChatCompletionResponse {
            id: request_id.to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: request.model.clone(),
            choices: vec![crate::types::Choice {
                index: 0,
                message: crate::types::ChatMessage {
                    role: "assistant".to_string(),
                    content: format!("Error: {}", e),
                },
                finish_reason: "error".to_string(),
            }],
            usage: None,
        }
    });

    tracing::info!(request_id = %request_id, "request complete");
    Json(response)
}
