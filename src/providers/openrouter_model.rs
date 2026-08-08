use async_trait::async_trait;
use std::collections::HashMap;
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, Choice, ChatMessage, Usage};
use super::{Model, ModelCapabilities, ModelPricing, TransportRequest, TransportResponse};

pub struct OpenRouterModel {
    pub model_id: String,
}

impl OpenRouterModel {
    pub fn new(model_id: String) -> Self {
        Self { model_id }
    }
}

#[async_trait]
impl Model for OpenRouterModel {
    fn id(&self) -> &str {
        &self.model_id
    }

    fn provider_name(&self) -> &str {
        "openrouter"
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            coding_score: 0.95,
            reasoning_score: 0.95,
            max_context_tokens: 200000,
            supports_tools: true,
            supports_streaming: true,
            supports_vision: true,
            supports_json_mode: true,
        }
    }

    fn pricing(&self) -> ModelPricing {
        ModelPricing {
            input_cost_per_1k: 0.003,
            output_cost_per_1k: 0.015,
        }
    }

    fn quota_remaining(&self) -> Option<f64> {
        None
    }

    fn format_request(&self, req: &ChatCompletionRequest, api_key: &str) -> anyhow::Result<TransportRequest> {
        let base_url = std::env::var("OPENROUTER_BASE_URL")
            .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());
        let url = format!("{}/chat/completions", base_url);
        
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), format!("Bearer {}", api_key));
        headers.insert("HTTP-Referer".to_string(), "https://github.com/anomalyco/opencode".to_string());
        headers.insert("X-Title".to_string(), "FusionRouter".to_string());
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        // The registry routes on `<provider-key>/` prefixes; strip the routing
        // prefix before forwarding so the upstream API receives a bare model
        // id (mirrors ZenModel::format_request).
        let api_model = req
            .model
            .strip_prefix("openrouter/")
            .unwrap_or(&req.model);

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

        Ok(TransportRequest {
            url,
            method: "POST".to_string(),
            headers,
            body,
        })
    }

    fn normalize_response(&self, resp: TransportResponse) -> anyhow::Result<ChatCompletionResponse> {
        let body = resp.body;
        let id = body["id"].as_str().unwrap_or("or-id").to_string();
        let model = body["model"].as_str().unwrap_or(&self.model_id).to_string();
        let created = body["created"].as_i64().unwrap_or_else(|| chrono::Utc::now().timestamp());

        let choices: Vec<Choice> = body["choices"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let content = super::message_content(c);
                        let finish_reason =
                            c["finish_reason"].as_str().unwrap_or("stop").to_string();
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

    fn choice_with(content: serde_json::Value, finish_reason: &str) -> serde_json::Value {
        serde_json::json!({
            "index": 0,
            "message": { "role": "assistant", "content": content },
            "finish_reason": finish_reason,
        })
    }

    #[test]
    fn test_normalize_extracts_native_tool_calls() {
        let model = OpenRouterModel::new("t".into());
        let resp = TransportResponse {
            status: 200,
            body: serde_json::json!({
                "id": "1", "model": "t", "created": 0, "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [
                            {
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "calculator",
                                    "arguments": "{\"expression\": \"2+2\"}"
                                }
                            }
                        ]
                    },
                    "finish_reason": "tool_calls"
                }],
            }),
        };
        let out = model.normalize_response(resp).unwrap();
        let calls = out.native_tool_calls.expect("native tool calls must be extracted");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "calculator");
        assert_eq!(calls[0].arguments["expression"], "2+2");
    }

    #[test]
    fn test_normalize_no_tool_calls_yields_none() {
        let model = OpenRouterModel::new("t".into());
        let resp = TransportResponse {
            status: 200,
            body: serde_json::json!({
                "id": "1", "model": "t", "created": 0, "object": "chat.completion",
                "choices": [choice_with(serde_json::Value::String("hi".into()), "stop")],
            }),
        };
        let out = model.normalize_response(resp).unwrap();
        assert!(out.native_tool_calls.is_none());
    }

    #[test]
    fn test_format_request_includes_tools_when_present() {
        use crate::types::ToolDefinition;
        let model = OpenRouterModel::new("t".into());
        let req = ChatCompletionRequest {
            model: "t".into(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: Some(vec![ToolDefinition {
                name: "calculator".into(),
                description: "calc".into(),
                parameters: None,
            }]),
            files: None,
            execution: None,
            output: None,
            strategy: None,
        };
        let transport_req = model.format_request(&req, "k").unwrap();
        assert_eq!(transport_req.body["tools"][0]["type"], "function");
        assert_eq!(
            transport_req.body["tools"][0]["function"]["name"],
            "calculator"
        );
        assert_eq!(
            transport_req.body["tools"][0]["function"]["description"],
            "calc"
        );
    }
}
