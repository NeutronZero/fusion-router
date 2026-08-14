use async_trait::async_trait;
use std::collections::HashMap;
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, Choice, ChatMessage, Usage};
use super::{Model, ModelCapabilities, ModelPricing, TransportRequest, TransportResponse};
use crate::config::CapabilityDescriptor;

/// A model backed by any OpenAI-compatible `/v1/chat/completions` endpoint.
///
/// This is the workhorse behind config-driven provider support — it covers
/// 75+ providers (DeepSeek, Groq, Cerebras, Fireworks, Together, xAI, etc.)
/// without any provider-specific code.
pub struct GenericOpenAIModel {
    pub model_id: String,
    pub provider_name: String,
    pub base_url: String,
    pub caps: ModelCapabilities,
    pub pricing: ModelPricing,
    pub prefix: String,
}

impl GenericOpenAIModel {
    pub fn new(
        model_id: String,
        provider_name: String,
        base_url: String,
        model_cfg: &CapabilityDescriptor,
        prefix: String,
    ) -> Self {
        let caps = ModelCapabilities {
            coding_score: model_cfg.coding_score.unwrap_or(0.8),
            reasoning_score: model_cfg.reasoning_score.unwrap_or(0.7),
            max_context_tokens: model_cfg.context_limit.unwrap_or(128_000),
            max_output_tokens: model_cfg.output_limit.unwrap_or(0),
            supports_tools: model_cfg.supports_tools.unwrap_or(true),
            supports_streaming: model_cfg.supports_streaming.unwrap_or(true),
            supports_vision: model_cfg.supports_vision.unwrap_or(false),
            supports_audio: model_cfg.supports_audio.unwrap_or(false),
            supports_pdf: model_cfg.supports_pdf.unwrap_or(false),
            supports_json_mode: model_cfg.supports_json_mode.unwrap_or(true),
            supports_thinking: model_cfg.supports_thinking.unwrap_or(false),
            supports_parallel_tools: model_cfg.supports_parallel_tools.unwrap_or(false),
            supports_structured_output: model_cfg.supports_structured_output.unwrap_or(false),
        };
        let pricing = ModelPricing {
            input_cost_per_1k: model_cfg.input_cost_per_1k.unwrap_or(crate::types::NanoUSD::ZERO),
            output_cost_per_1k: model_cfg.output_cost_per_1k.unwrap_or(crate::types::NanoUSD::ZERO),
        };
        Self { model_id, provider_name, base_url, caps, pricing, prefix }
    }
}

#[async_trait]
impl Model for GenericOpenAIModel {
    fn id(&self) -> &str {
        &self.model_id
    }

    fn provider_name(&self) -> &str {
        &self.provider_name
    }

    fn capabilities(&self) -> ModelCapabilities {
        self.caps.clone()
    }

    fn pricing(&self) -> ModelPricing {
        self.pricing.clone()
    }

    fn quota_remaining(&self) -> Option<f64> {
        None
    }

    fn format_request(&self, req: &ChatCompletionRequest, api_key: &str) -> anyhow::Result<TransportRequest> {
        let url = format!("{}/chat/completions", self.base_url);

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), format!("Bearer {}", api_key));
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        // Strip the routing prefix (e.g. "deepseek/") before forwarding.
        let api_model = if self.prefix.is_empty() {
            req.model.clone()
        } else {
            req.model.strip_prefix(&self.prefix).unwrap_or(&req.model).to_string()
        };

        let mut body = serde_json::json!({
            "model": api_model,
            "messages": req.messages,
            "stream": req.stream,
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
        });
        if let Some(tools) = &req.tools {
            body["tools"] = serde_json::json!(super::tool_definitions_wire(tools));
        }

        Ok(TransportRequest { url, method: "POST".to_string(), headers, body })
    }

    fn normalize_response(&self, resp: TransportResponse) -> anyhow::Result<ChatCompletionResponse> {
        let body = resp.body;
        let id = body["id"].as_str().unwrap_or("gen-id").to_string();
        let model = body["model"].as_str().unwrap_or(&self.model_id).to_string();
        let created = body["created"].as_i64().unwrap_or_else(|| chrono::Utc::now().timestamp());

        let choices: Vec<Choice> = body["choices"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let content = super::message_content(c);
                        let finish_reason = c["finish_reason"].as_str().unwrap_or("stop").to_string();
                        super::ensure_non_truncated(c, &content)?;
                        Ok(Choice {
                            index: i as u32,
                            message: ChatMessage {
                                role: c["message"]["role"].as_str().unwrap_or("assistant").to_string(),
                                content,
                            },
                            finish_reason,
                        })
                    })
                    .collect::<anyhow::Result<Vec<Choice>>>()
            })
            .transpose()?
            .unwrap_or_default();

        let usage = body["usage"].as_object().map(|u| Usage {
            prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
        });

        let native_tool_calls = super::native_tool_calls_from(&body, "choices", 0);

        Ok(ChatCompletionResponse {
            id,
            object: "chat.completion".to_string(),
            created,
            model,
            choices,
            usage,
            native_tool_calls,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportResponse;

    fn test_model_config() -> CapabilityDescriptor {
        CapabilityDescriptor {
            name: Some("Test Model".into()),
            context_limit: Some(64_000),
            output_limit: Some(4_096),
            coding_score: Some(0.9),
            reasoning_score: Some(0.85),
            supports_tools: Some(true),
            supports_streaming: Some(true),
            supports_vision: Some(false),
            supports_json_mode: Some(true),
            input_cost_per_1k: Some(crate::types::NanoUSD::from_nanos(1_000_000)),
            output_cost_per_1k: Some(crate::types::NanoUSD::from_nanos(2_000_000)),
            ..Default::default()
        }
    }

    #[test]
    fn test_model_uses_config_values() {
        let model = GenericOpenAIModel::new(
            "gpt-4o".into(),
            "openai".into(),
            "https://api.openai.com/v1".into(),
            &test_model_config(),
            "openai/".into(),
        );
        let caps = model.capabilities();
        assert_eq!(caps.max_context_tokens, 64_000);
        assert!(!caps.supports_vision);
        assert!(caps.supports_tools);
        let pricing = model.pricing();
        assert_eq!(pricing.input_cost_per_1k, crate::types::NanoUSD::from_nanos(1_000_000));
        assert_eq!(pricing.output_cost_per_1k, crate::types::NanoUSD::from_nanos(2_000_000));
    }

    #[test]
    fn test_format_request_strips_prefix() {
        let model = GenericOpenAIModel::new(
            "gpt-4o".into(),
            "openai".into(),
            "https://api.openai.com/v1".into(),
            &CapabilityDescriptor::default(),
            "openai/".into(),
        );
        let req = ChatCompletionRequest {
            model: "openai/gpt-4o".into(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            files: None,
            execution: None,
            output: None,
            strategy: None,
        };
        let transport_req = model.format_request(&req, "sk-test").unwrap();
        assert_eq!(transport_req.body["model"], "gpt-4o");
        assert_eq!(transport_req.url, "https://api.openai.com/v1/chat/completions");
        assert_eq!(
            transport_req.headers.get("Authorization").unwrap(),
            "Bearer sk-test"
        );
    }

    #[test]
    fn test_format_request_no_prefix() {
        let model = GenericOpenAIModel::new(
            "deepseek-chat".into(),
            "deepseek".into(),
            "https://api.deepseek.com/v1".into(),
            &CapabilityDescriptor::default(),
            "".into(),
        );
        let req = ChatCompletionRequest {
            model: "deepseek-chat".into(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            files: None,
            execution: None,
            output: None,
            strategy: None,
        };
        let transport_req = model.format_request(&req, "sk-test").unwrap();
        assert_eq!(transport_req.body["model"], "deepseek-chat");
    }

    #[test]
    fn test_normalize_standard_response() {
        let model = GenericOpenAIModel::new(
            "test".into(),
            "test".into(),
            "http://localhost".into(),
            &CapabilityDescriptor::default(),
            "".into(),
        );
        let resp = TransportResponse {
            status: 200,
            body: serde_json::json!({
                "id": "cmpl-123",
                "model": "test",
                "created": 1000,
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "hello" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15
                }
            }),
        };
        let out = model.normalize_response(resp).unwrap();
        assert_eq!(out.id, "cmpl-123");
        assert_eq!(out.choices[0].message.content, "hello");
        assert_eq!(out.usage.unwrap().total_tokens, 15);
    }

    #[test]
    fn test_normalize_extracts_tool_calls() {
        let model = GenericOpenAIModel::new(
            "test".into(),
            "test".into(),
            "http://localhost".into(),
            &CapabilityDescriptor::default(),
            "".into(),
        );
        let resp = TransportResponse {
            status: 200,
            body: serde_json::json!({
                "id": "cmpl-1", "model": "test", "created": 0,
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "search",
                                "arguments": "{\"query\": \"rust\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }),
        };
        let out = model.normalize_response(resp).unwrap();
        let calls = out.native_tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "search");
        assert_eq!(calls[0].arguments["query"], "rust");
    }
}
