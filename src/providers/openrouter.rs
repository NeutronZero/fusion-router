use async_trait::async_trait;
use reqwest::Client;

use super::Provider;
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, Choice, ChatMessage, Usage};

pub struct OpenRouterProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl OpenRouterProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: std::env::var("OPENROUTER_BASE_URL")
                .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string()),
        }
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    fn name(&self) -> &str {
        "openrouter"
    }

    async fn chat_completion(&self, request: &ChatCompletionRequest) -> anyhow::Result<ChatCompletionResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://github.com/anomalyco/opencode")
            .header("X-Title", "FusionRouter")
            .json(&serde_json::json!({
                "model": request.model,
                "messages": request.messages,
                "stream": false,
                "temperature": request.temperature,
                "max_tokens": request.max_tokens,
            }))
            .send()
            .await?;

        let body: serde_json::Value = resp.json().await?;

        let id = body["id"].as_str().unwrap_or("or-id").to_string();
        let model = body["model"].as_str().unwrap_or(&request.model).to_string();
        let created = body["created"].as_i64().unwrap_or_else(|| chrono::Utc::now().timestamp());

        let choices: Vec<Choice> = body["choices"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .enumerate()
                    .map(|(i, c)| Choice {
                        index: i as u32,
                        message: ChatMessage {
                            role: c["message"]["role"].as_str().unwrap_or("assistant").to_string(),
                            content: c["message"]["content"].as_str().unwrap_or("").to_string(),
                        },
                        finish_reason: c["finish_reason"].as_str().unwrap_or("stop").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let usage = body["usage"].as_object().map(|u| Usage {
            prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
        });

        Ok(ChatCompletionResponse {
            id,
            object: "chat.completion".to_string(),
            created,
            model,
            choices,
            usage,
        })
    }
}
