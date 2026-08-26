use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// Config-driven custom headers merged onto every outgoing
    /// `TransportRequest`. Explicitly configured headers WIN over headers the
    /// inner model sets (including `Authorization`); unset keys leave the
    /// model's own headers untouched.
    extra_headers: parking_lot::RwLock<HashMap<String, String>>,
}

impl Provider {
    pub fn new(model: Box<dyn Model>, transport: Box<dyn Transport>, api_key: String) -> Self {
        Self {
            model,
            transport,
            api_key,
            extra_headers: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    /// Replaces the set of custom headers injected into outgoing requests.
    pub fn set_extra_headers(&self, headers: HashMap<String, String>) {
        *self.extra_headers.write() = headers;
    }

    fn apply_extra_headers(&self, req: &mut TransportRequest) {
        for (k, v) in self.extra_headers.read().iter() {
            req.headers.insert(k.clone(), v.clone());
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
        let mut req = self.model.format_request(request, &self.api_key)?;
        self.apply_extra_headers(&mut req);
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
        self.apply_extra_headers(&mut transport_req);
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
                    drain_sse_events(buf)
                }
                Err(e) => vec![Err(anyhow::anyhow!("Transport error: {}", e))],
            };
            async move { Some(chunks) }
        });

        Ok(Box::pin(framed.flat_map(stream::iter)))
    }
}

// ---------------------------------------------------------------------------
// SSE framing
// ---------------------------------------------------------------------------

/// Hard cap for the buffered SSE scan buffer. A hostile/broken upstream that
/// never emits an event boundary must not grow the buffer without bound.
pub const MAX_SSE_SCAN_BUFFER_BYTES: usize = 1024 * 1024;

/// Locates the earliest SSE event delimiter in `buf`, supporting both LF
/// (`\n\n`) and CRLF (`\r\n\r\n`) framings. Returns `(start, delim_len)`.
fn find_sse_boundary(buf: &str) -> Option<(usize, usize)> {
    let lf = buf.find("\n\n").map(|p| (p, 2usize));
    let crlf = buf.find("\r\n\r\n").map(|p| (p, 4usize));
    match (lf, crlf) {
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (Some(a), Some(b)) => {
            if a.0 <= b.0 {
                Some(a)
            } else {
                Some(b)
            }
        }
        (None, None) => None,
    }
}

/// Extracts `data:` payload lines from one raw SSE event. Per the SSE spec
/// only `data:` lines carry payload; comments (`:`) and other fields
/// (`event:`, `id:`, `retry:`) are ignored — some upstreams emit keep-alive
/// comments between chunks. Handles both `\n` and `\r\n` line endings.
fn parse_event_payloads(raw: &str) -> Vec<String> {
    let mut payloads: Vec<String> = Vec::new();
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            let data = rest.strip_prefix(' ').unwrap_or(rest);
            if !data.trim().is_empty() && data != "[DONE]" {
                payloads.push(data.to_string());
            }
        }
    }
    payloads
}

/// Drains every complete event from `buf`, parsing payloads into chunks.
/// If no boundary is present and the buffer exceeds
/// [`MAX_SSE_SCAN_BUFFER_BYTES`], emits a single error and clears the buffer
/// (fail-closed rather than unbounded growth).
fn drain_sse_events(buf: &mut String) -> Vec<anyhow::Result<ChatStreamChunk>> {
    let mut chunks: Vec<anyhow::Result<ChatStreamChunk>> = Vec::new();
    loop {
        match find_sse_boundary(buf) {
            Some((pos, delim_len)) => {
                let raw = buf[..pos].trim().to_string();
                buf.drain(..pos + delim_len);
                for data in parse_event_payloads(&raw) {
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
            None => {
                if buf.len() > MAX_SSE_SCAN_BUFFER_BYTES {
                    tracing::warn!(
                        size = buf.len(),
                        cap = MAX_SSE_SCAN_BUFFER_BYTES,
                        "SSE frame exceeded scan buffer cap; dropping buffered bytes"
                    );
                    chunks.push(Err(anyhow::anyhow!(
                        "streamed SSE frame exceeded maximum buffered size"
                    )));
                    buf.clear();
                }
                break;
            }
        }
    }
    chunks
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

    #[test]
    fn test_find_sse_boundary_lf() {
        assert_eq!(super::find_sse_boundary("a\n\nb"), Some((1, 2)));
    }

    #[test]
    fn test_find_sse_boundary_crlf() {
        assert_eq!(super::find_sse_boundary("a\r\n\r\nb"), Some((1, 4)));
    }

    #[test]
    fn test_find_sse_boundary_mixed_prefers_earliest() {
        // LF event first, CRLF later.
        assert_eq!(super::find_sse_boundary("a\n\nb\r\n\r\nc"), Some((1, 2)));
        // CRLF first, LF later.
        assert_eq!(super::find_sse_boundary("a\r\n\r\nb\n\nc"), Some((1, 4)));
    }

    #[test]
    fn test_find_sse_boundary_none() {
        assert_eq!(super::find_sse_boundary(""), None);
        assert_eq!(super::find_sse_boundary("\r\n"), None);
        assert_eq!(super::find_sse_boundary("no delimiter yet"), None);
    }

    #[test]
    fn test_parse_event_payloads_ignores_comments_and_other_fields() {
        let raw = ": keep-alive comment\nevent: message\nid: 42\ndata: {\"a\":1}\nretry: 100";
        assert_eq!(super::parse_event_payloads(raw), vec![r#"{"a":1}"#]);
    }

    #[test]
    fn test_parse_event_payloads_handles_crlf_lines_and_done() {
        let raw = "data: {\"x\":1}\r\ndata: [DONE]\r\ndata: {\"y\":2}\r\n";
        assert_eq!(
            super::parse_event_payloads(raw),
            vec![r#"{"x":1}"#, r#"{"y":2}"#]
        );
    }

    #[test]
    fn test_drain_sse_events_parses_crlf_framed_chunks() {
        let mut buf = String::from(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\r\n\r\n\
             data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\r\n\r\n",
        );
        let chunks = super::drain_sse_events(&mut buf);
        assert_eq!(chunks.len(), 2);
        let first = chunks[0].as_ref().unwrap();
        assert_eq!(first.content.as_deref(), Some("hi"));
        let second = chunks[1].as_ref().unwrap();
        assert_eq!(second.finish_reason.as_deref(), Some("stop"));
        assert!(buf.is_empty(), "CRLF-framed buffer must be fully drained");
    }

    #[test]
    fn test_drain_sse_events_partial_event_stays_buffered() {
        let mut buf = String::from("data: {\"choices\"");
        assert!(super::drain_sse_events(&mut buf).is_empty());
        assert_eq!(buf, "data: {\"choices\"", "incomplete frame stays buffered");
    }

    #[test]
    fn test_drain_sse_events_caps_unbounded_buffer() {
        let mut buf = "x".repeat(super::MAX_SSE_SCAN_BUFFER_BYTES + 1);
        buf.push_str("data: {}"); // still no boundary
        let chunks = super::drain_sse_events(&mut buf);
        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].is_err(),
            "oversized unterminated frame must error"
        );
        assert!(buf.is_empty(), "buffer must be reset after the cap trips");
    }
}
