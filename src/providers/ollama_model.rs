use async_trait::async_trait;
use std::collections::HashMap;
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, Choice, ChatMessage, Usage};
use super::{Model, ModelCapabilities, ModelPricing, TransportRequest, TransportResponse};

pub struct OllamaModel {
    pub model_id: String,
}

impl OllamaModel {
    pub fn new(model_id: String) -> Self {
        Self { model_id }
    }
}

#[async_trait]
impl Model for OllamaModel {
    fn id(&self) -> &str {
        &self.model_id
    }

    fn provider_name(&self) -> &str {
        "ollama"
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            coding_score: 0.6,
            reasoning_score: 0.5,
            max_context_tokens: 8192,
            supports_tools: false,
            supports_streaming: true,
            supports_vision: false,
            supports_json_mode: false,
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

    fn format_request(&self, req: &ChatCompletionRequest, _api_key: &str) -> anyhow::Result<TransportRequest> {
        let base_url = std::env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434/api".to_string());
        let url = format!("{}/chat", base_url);
        
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let body = serde_json::json!({
            "model": req.model,
            "messages": req.messages,
            "stream": req.stream,
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
        let id = body["id"].as_str().unwrap_or("ollama-id").to_string();
        let model = body["model"].as_str().unwrap_or(&self.model_id).to_string();
        let created = body["created_at"]
            .as_str()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .map(|dt| dt.timestamp())
            .unwrap_or_else(|| chrono::Utc::now().timestamp());

        let content = body["message"]["content"].as_str().unwrap_or("").to_string();
        let role = body["message"]["role"].as_str().unwrap_or("assistant").to_string();

        let choices = vec![Choice {
            index: 0,
            message: ChatMessage { role, content },
            finish_reason: "stop".to_string(),
        }];

        let usage = Usage {
            prompt_tokens: body["prompt_eval_count"].as_u64().unwrap_or(0) as u32,
            completion_tokens: body["eval_count"].as_u64().unwrap_or(0) as u32,
            total_tokens: (body["prompt_eval_count"].as_u64().unwrap_or(0) + body["eval_count"].as_u64().unwrap_or(0)) as u32,
        };

        Ok(ChatCompletionResponse {
            id,
            object: "chat.completion".to_string(),
            created,
            model,
            choices,
            usage: Some(usage),
        })
    }
}
