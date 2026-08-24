use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

pub mod circuit_breaker;
pub mod circuit_breaking_provider;
pub mod factory;
pub mod generic_openai_model;
pub mod ollama;
pub mod ollama_model;
pub mod openrouter;
pub mod openrouter_model;
pub mod provider_with_headers;
pub mod registry;
pub mod router;
pub mod zen;
pub mod zen_model;

#[allow(unused_imports)]
pub use registry::ProviderRegistry;

use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ChatStreamChunk, NanoUSD};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub coding_score: f32,
    pub reasoning_score: f32,
    pub max_context_tokens: u32,
    pub max_output_tokens: u32,
    pub supports_tools: bool,
    pub supports_streaming: bool,
    pub supports_vision: bool,
    pub supports_audio: bool,
    pub supports_pdf: bool,
    pub supports_json_mode: bool,
    pub supports_thinking: bool,
    pub supports_parallel_tools: bool,
    pub supports_structured_output: bool,
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            coding_score: 0.0,
            reasoning_score: 0.0,
            max_context_tokens: 0,
            max_output_tokens: 0,
            supports_tools: false,
            supports_streaming: false,
            supports_vision: false,
            supports_audio: false,
            supports_pdf: false,
            supports_json_mode: false,
            supports_thinking: false,
            supports_parallel_tools: false,
            supports_structured_output: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input_cost_per_1k: NanoUSD,
    pub output_cost_per_1k: NanoUSD,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelRequirements {
    pub min_context_tokens: Option<u32>,
    pub min_coding_score: Option<f32>,
    pub min_reasoning_score: Option<f32>,
    pub requires_tools: bool,
    pub requires_streaming: bool,
    pub requires_vision: bool,
    pub max_cost_per_1k_tokens: Option<NanoUSD>,
    pub preferred_provider: Option<String>,
}

impl ModelRequirements {
    pub fn matches(&self, capabilities: &ModelCapabilities, pricing: &ModelPricing) -> bool {
        if self.requires_tools && !capabilities.supports_tools {
            return false;
        }
        if self.requires_streaming && !capabilities.supports_streaming {
            return false;
        }
        if self.requires_vision && !capabilities.supports_vision {
            return false;
        }
        if let Some(min_ctx) = self.min_context_tokens {
            if capabilities.max_context_tokens < min_ctx {
                return false;
            }
        }
        if let Some(min_code) = self.min_coding_score {
            if capabilities.coding_score < min_code {
                return false;
            }
        }
        if let Some(min_reason) = self.min_reasoning_score {
            if capabilities.reasoning_score < min_reason {
                return false;
            }
        }
        if let Some(max_cost) = self.max_cost_per_1k_tokens {
            if (pricing.input_cost_per_1k + pricing.output_cost_per_1k) > max_cost {
                return false;
            }
        }
        true
    }
}

/// Extracts the assistant message content from a completion choice. Some
/// providers return `content` as an array of parts instead of a plain string;
/// non-string content must never silently collapse to an empty answer.
pub fn message_content(choice: &serde_json::Value) -> String {
    match &choice["message"]["content"] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => {
            parts.iter().filter_map(|p| p.as_str()).collect::<String>()
        }
        _ => String::new(),
    }
}

/// Returns an error when the provider finished with `length` (truncation) and
/// produced no usable content — silently surfacing an empty completion is
/// worse than a retriable failure.
pub fn ensure_non_truncated(choice: &serde_json::Value, content: &str) -> anyhow::Result<()> {
    if content.is_empty() && choice["finish_reason"].as_str() == Some("length") {
        anyhow::bail!(
            "completion truncated (finish_reason=length) with empty content; increase max_tokens"
        );
    }
    Ok(())
}

/// Extracts provider-native tool calls from a transport response body.
///
/// Law 7 / ADR-037: tool execution is fed ONLY from these structured
/// OpenAI-compatible wire shape for tool definitions
/// (`{"type": "function", "function": {name, description, parameters}}`).
/// `ToolDefinition` itself is the domain model; this maps it onto the
/// provider transport contract (ADR-037).
pub fn tool_definitions_wire(tools: &[crate::types::ToolDefinition]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                }
            })
        })
        .collect()
}

/// `tool_calls` — model output text is never parsed for tool invocation.
///
/// `container` names the node holding the message: `"choices"` (OpenAI wire
/// shape, index `choice_index`) or `"message"` (Ollama wire shape, index -1).
pub fn native_tool_calls_from(
    body: &serde_json::Value,
    container: &str,
    choice_index: i32,
) -> Option<Vec<crate::types::ToolCall>> {
    let holder = if container == "choices" {
        body[container]
            .as_array()
            .and_then(|arr| arr.get(choice_index as usize))
            .map(|c| &c["message"])
            .unwrap_or(&serde_json::Value::Null)
    } else {
        &body[container]
    };

    let calls = holder["tool_calls"].as_array()?;
    let parsed: Vec<crate::types::ToolCall> = calls
        .iter()
        .filter_map(|tc| {
            let name = tc["function"]["name"].as_str()?;
            let arguments = tc["function"]["arguments"]
                .as_str()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .unwrap_or(serde_json::Value::Object(Default::default()));
            Some(crate::types::ToolCall {
                id: tc["id"].as_str().unwrap_or("").to_string(),
                name: name.to_string(),
                arguments,
            })
        })
        .collect();
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

#[async_trait]
pub trait Model: Send + Sync {
    fn id(&self) -> &str;
    fn provider_name(&self) -> &str;
    fn capabilities(&self) -> ModelCapabilities;
    fn pricing(&self) -> ModelPricing;
    fn quota_remaining(&self) -> Option<f64>;

    // Method to format a request for this model
    fn format_request(
        &self,
        req: &ChatCompletionRequest,
        api_key: &str,
    ) -> anyhow::Result<TransportRequest>;

    // Method to normalize a response for this model
    fn normalize_response(&self, resp: TransportResponse)
        -> anyhow::Result<ChatCompletionResponse>;
}

pub use crate::transport::HttpTransport;
pub use crate::transport::{Transport, TransportRequest, TransportResponse};

#[async_trait]
pub trait ChatProvider: Send + Sync {
    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<ChatCompletionResponse>;
    fn name(&self) -> &str;

    async fn chat_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<ChatStreamChunk>>> {
        let response = self.chat_completion(request).await?;
        let content = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        let usage = response.usage;
        Ok(Box::pin(stream::once(async move {
            Ok(ChatStreamChunk {
                content: Some(content),
                finish_reason: Some("stop".to_string()),
                usage,
            })
        })))
    }
}

pub struct Provider {
    pub model: Box<dyn Model>,
    pub transport: Box<dyn Transport>,
    pub api_key: String,
}

impl Provider {
    pub fn new(model: Box<dyn Model>, transport: Box<dyn Transport>, api_key: String) -> Self {
        Self {
            model,
            transport,
            api_key,
        }
    }
}

#[async_trait]
impl ChatProvider for Provider {
    fn name(&self) -> &str {
        self.model.provider_name()
    }

    #[tracing::instrument(skip(self, request), fields(provider = %self.name(), model = %request.model))]
    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<ChatCompletionResponse> {
        let req = self.model.format_request(request, &self.api_key)?;
        let resp = self
            .transport
            .send(req)
            .await
            .map_err(|e| anyhow::anyhow!("Transport error: {}", e))?;
        self.model.normalize_response(resp)
    }

    #[tracing::instrument(skip(self, request), fields(provider = %self.name(), model = %request.model))]
    async fn chat_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<ChatStreamChunk>>> {
        let mut transport_req = self.model.format_request(request, &self.api_key)?;
        transport_req.body["stream"] = serde_json::json!(true);
        let stream = self
            .transport
            .stream(transport_req)
            .await
            .map_err(|e| anyhow::anyhow!("Transport error: {}", e))?;

        let framed = stream.scan(String::new(), |buf, event| {
            let chunks = match event {
                Ok(event) => {
                    buf.push_str(&event.data);
                    let mut chunks = Vec::new();
                    while let Some(pos) = buf.find("\n\n") {
                        let raw = buf[..pos].trim().to_string();
                        buf.drain(..=pos + 1);
                        // Per the SSE spec only `data:` lines carry payload;
                        // comments (`:`) and other fields (`event:`, `id:`,
                        // `retry:`) are ignored — some upstreams emit
                        // keep-alive comments between chunks.
                        let mut payloads: Vec<String> = Vec::new();
                        for line in raw.lines() {
                            if let Some(rest) = line.strip_prefix("data:") {
                                let data = rest.strip_prefix(' ').unwrap_or(rest);
                                if !data.trim().is_empty() && data != "[DONE]" {
                                    payloads.push(data.to_string());
                                }
                            }
                        }
                        for data in payloads {
                            match ChatStreamChunk::from_sse_data(&data) {
                                Ok(Some(chunk)) => chunks.push(Ok(chunk)),
                                Ok(None) => {}
                                Err(e) => {
                                    tracing::warn!(raw = %data, "SSE parse error");
                                    chunks.push(Err(e))
                                }
                            }
                        }
                    }
                    chunks
                }
                Err(e) => vec![Err(anyhow::anyhow!("Transport error: {}", e))],
            };
            async move { Some(chunks) }
        });

        Ok(Box::pin(framed.flat_map(stream::iter)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_caps() -> ModelCapabilities {
        ModelCapabilities {
            coding_score: 0.9,
            reasoning_score: 0.85,
            max_context_tokens: 128_000,
            max_output_tokens: 0,
            supports_tools: true,
            supports_streaming: true,
            supports_vision: true,
            supports_audio: false,
            supports_pdf: false,
            supports_json_mode: true,
            supports_thinking: false,
            supports_parallel_tools: false,
            supports_structured_output: false,
        }
    }

    fn base_pricing() -> ModelPricing {
        ModelPricing {
            input_cost_per_1k: NanoUSD::from_nanos(3_000_000_000),
            output_cost_per_1k: NanoUSD::from_nanos(15_000_000_000),
        }
    }

    #[test]
    fn test_matches_tools_req_fails_when_unsupported() {
        let req = ModelRequirements {
            requires_tools: true,
            ..Default::default()
        };
        let caps = ModelCapabilities {
            supports_tools: false,
            ..base_caps()
        };
        assert!(!req.matches(&caps, &base_pricing()));
    }

    #[test]
    fn test_matches_context_window_fails_when_too_small() {
        let req = ModelRequirements {
            min_context_tokens: Some(200_000),
            ..Default::default()
        };
        assert!(!req.matches(&base_caps(), &base_pricing()));
    }

    #[test]
    fn test_matches_cost_ceiling_fails_when_exceeded() {
        let req = ModelRequirements {
            max_cost_per_1k_tokens: Some(NanoUSD::from_nanos(10_000_000_000)),
            ..Default::default()
        };
        assert!(!req.matches(&base_caps(), &base_pricing()));
    }

    #[test]
    fn test_matches_all_satisfied_returns_true() {
        let req = ModelRequirements {
            requires_tools: true,
            requires_streaming: true,
            min_context_tokens: Some(32_000),
            min_coding_score: Some(0.7),
            max_cost_per_1k_tokens: Some(NanoUSD::from_nanos(50_000_000_000)),
            ..Default::default()
        };
        assert!(req.matches(&base_caps(), &base_pricing()));
    }

    #[test]
    fn test_matches_default_accepts_anything() {
        let req = ModelRequirements::default();
        assert!(req.matches(&base_caps(), &base_pricing()));
        let minimal = ModelCapabilities {
            coding_score: 0.1,
            reasoning_score: 0.1,
            max_context_tokens: 1_000,
            max_output_tokens: 0,
            supports_tools: false,
            supports_streaming: false,
            supports_vision: false,
            supports_audio: false,
            supports_pdf: false,
            supports_json_mode: false,
            supports_thinking: false,
            supports_parallel_tools: false,
            supports_structured_output: false,
        };
        assert!(req.matches(&minimal, &base_pricing()));
    }

    #[test]
    fn test_native_tool_calls_openai_choices_shape() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "ok",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "calculator",
                            "arguments": "{\"a\": 2, \"b\": 3}"
                        }
                    }]
                }
            }]
        });
        let calls = super::native_tool_calls_from(&body, "choices", 0).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "calculator");
        assert_eq!(calls[0].arguments["a"], 2);
        assert_eq!(calls[0].arguments["b"], 3);
    }

    #[test]
    fn test_native_tool_calls_ollama_message_shape() {
        let body = serde_json::json!({
            "message": {
                "content": "ok",
                "tool_calls": [{
                    "function": {
                        "name": "calculator",
                        "arguments": "{\"a\": 1, \"b\": 1}"
                    }
                }]
            }
        });
        let calls = super::native_tool_calls_from(&body, "message", -1).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "calculator");
        assert_eq!(calls[0].arguments["a"], 1);
    }

    #[test]
    fn test_native_tool_calls_malformed_arguments_yield_empty_object() {
        let body = serde_json::json!({
            "message": {
                "tool_calls": [{
                    "function": {
                        "name": "calculator",
                        "arguments": "not-json"
                    }
                }]
            }
        });
        let calls = super::native_tool_calls_from(&body, "message", -1).unwrap();
        assert_eq!(calls[0].name, "calculator");
        assert!(calls[0].arguments.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_native_tool_calls_none_when_absent() {
        let body = serde_json::json!({ "choices": [{ "message": { "content": "plain" } }] });
        assert!(super::native_tool_calls_from(&body, "choices", 0).is_none());
        let empty = serde_json::json!({ "message": { "tool_calls": [] } });
        assert!(super::native_tool_calls_from(&empty, "message", -1).is_none());
    }
}
