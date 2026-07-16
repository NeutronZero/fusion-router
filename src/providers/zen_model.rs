use async_trait::async_trait;
use std::collections::HashMap;
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, Choice, ChatMessage, Usage};
use super::{Model, ModelCapabilities, ModelPricing, TransportRequest, TransportResponse};

pub struct ZenModel {
    pub model_id: String,
}

impl ZenModel {
    pub fn new(model_id: String) -> Self {
        Self { model_id }
    }
}

#[async_trait]
impl Model for ZenModel {
    fn id(&self) -> &str {
        &self.model_id
    }

    fn provider_name(&self) -> &str {
        "opencode-zen"
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            coding_score: 0.9,
            reasoning_score: 0.85,
            max_context_tokens: 32768,
            supports_tools: true,
            supports_streaming: true,
            supports_vision: false,
            supports_json_mode: true,
        }
    }

    fn pricing(&self) -> ModelPricing {
        ModelPricing {
            input_cost_per_1k: 0.0,
            output_cost_per_1k: 0.0,
        }
    }

    fn quota_remaining(&self) -> Option<f64> {
        None
    }

    fn format_request(&self, req: &ChatCompletionRequest, api_key: &str) -> anyhow::Result<TransportRequest> {
        let base_url = std::env::var("OPENCODEZEN_BASE_URL")
            .unwrap_or_else(|_| "https://api.opencode.ai/v1".to_string());
        let url = format!("{}/chat/completions", base_url);
        
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), format!("Bearer {}", api_key));
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let body = serde_json::json!({
            "model": req.model,
            "messages": req.messages,
            "stream": req.stream,
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
        });

        Ok(TransportRequest {
            url,
            method: "POST".to_string(),
            headers,
            body,
        })
    }

    fn normalize_response(&self, resp: TransportResponse) -> anyhow::Result<ChatCompletionResponse> {
        let body = resp.body;
        let id = body["id"].as_str().unwrap_or("zen-id").to_string();
        let model = body["model"].as_str().unwrap_or(&self.model_id).to_string();
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
