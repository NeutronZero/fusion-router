use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub mod anthropic;
pub mod artifact;
pub mod error;
pub mod execution;
pub mod execution_context;

pub use anthropic::{AnthropicMessagesRequest, AnthropicMessagesResponse};
pub use artifact::ArtifactKind;
pub use error::{PipelineStage, RouterError};

// Re-export core types from fusion-types (the canonical source)
pub use fusion_types::{
    ChatMessage,
    // Errors
    CompilerError,
    ComplexityLevel,
    EvidenceSnapshot,
    ExecutionEdge,
    // Execution graph
    ExecutionGraph,
    // Execution intent
    ExecutionIntent,
    ExecutionNode,
    ExecutionNodeKind,
    ExecutionRecord,
    ExecutionResult,
    FallbackConfig,
    GraphMetadata,
    IREdge,
    IRMetadata,
    IRNode,
    IRNodeKind,
    Intent,
    ModelCatalog,
    NanoUSD,
    NodeExecContext,
    NodeExecutionResult,
    // Runtime state
    NodeState,
    OutputPreferences,
    // Policy
    Policy,
    PolicyAction,
    PolicyCondition,
    ProviderLimit,
    Quota,
    ReservationId,
    RetryPolicy,
    SchedulerError,
    // Strategy
    StrategyKind,
    ToolCall,
    // Shared value objects
    Usage,
    // IR types
    WorkflowIR,
};

// ---------------------------------------------------------------------------
// HTTP/API types (stay in src/ — depend on providers/resource)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(default)]
    pub files: Option<Vec<FileRef>>,
    #[serde(default)]
    pub execution: Option<ExecutionIntent>,
    #[serde(default)]
    pub output: Option<OutputPreferences>,
    #[serde(default)]
    pub strategy: Option<RequestStrategy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestStrategy {
    #[serde(default = "default_strategy_kind")]
    pub kind: String,
    #[serde(default = "default_strategy_count")]
    pub count: u32,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default = "default_tool_rounds")]
    pub max_tool_rounds: u64,
}

fn default_strategy_kind() -> String {
    "Consensus".into()
}

fn default_strategy_count() -> u32 {
    3
}

fn default_tool_rounds() -> u64 {
    8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub parameters: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRef {
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSnapshot {
    pub messages: Vec<ChatMessage>,
    pub files: Vec<FileRef>,
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: u32,
    pub temperature: f32,
}

// Requirements with model_requirements (depends on crate::providers)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirements {
    pub intent_classification: Intent,
    pub complexity: ComplexityLevel,
    pub has_files: bool,
    pub context_window: u64,
    pub original_text: String,
    #[serde(default)]
    pub execution_intent: Option<ExecutionIntent>,
    #[serde(default)]
    pub output_preferences: Option<OutputPreferences>,
    #[serde(default)]
    pub requested_strategy: Option<RequestStrategy>,
    #[serde(default)]
    pub requested_model: Option<String>,
    #[serde(default, skip)]
    pub model_requirements: Option<crate::providers::ModelRequirements>,
}

// ExecutionInstance with BudgetEnvelope (depends on crate::resource)
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionInstance {
    pub instance_id: Uuid,
    pub graph: Arc<ExecutionGraph>,
    pub node_states: HashMap<Uuid, NodeState>,
    pub outputs: HashMap<Uuid, serde_json::Value>,
    pub reservation_id: Uuid,
    pub created_at: i64,
    pub terminal_node_id: Option<Uuid>,
    pub final_output: Option<serde_json::Value>,
    #[serde(skip)]
    pub budget_envelope: Option<crate::resource::BudgetEnvelope>,
}

/// Runtime ABI contract between providers and executor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
    #[serde(default)]
    pub native_tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatStreamChunk {
    pub content: Option<String>,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
}

impl ChatStreamChunk {
    /// Parses one OpenAI-compatible SSE `data:` payload.
    ///
    /// `finish_reason` and `usage` are parsed whenever present, independent of
    /// `delta.content`: OpenAI's final chunk carries an EMPTY delta object
    /// (`{"delta":{},"finish_reason":"stop","usage":{...}}`), which must not
    /// be dropped. A missing `content` field is treated like an empty one.
    pub fn from_sse_data(data: &str) -> anyhow::Result<Option<Self>> {
        let trimmed = data.trim();
        if trimmed.is_empty() || trimmed == "[DONE]" {
            return Ok(None);
        }
        let json_str = trimmed.strip_prefix("data: ").unwrap_or(trimmed);
        let body: serde_json::Value = serde_json::from_str(json_str)?;
        let content = body["choices"][0]["delta"]["content"]
            .as_str()
            .map(|s| s.to_string());
        let finish_reason = body["choices"][0]["finish_reason"]
            .as_str()
            .map(|s| s.to_string());
        let usage = parse_usage(body.get("usage"));

        // Pure keep-alive frame (`delta: {}` with no finish/usage) — no payload.
        if content.as_deref().unwrap_or("").is_empty() && finish_reason.is_none() && usage.is_none()
        {
            return Ok(None);
        }
        Ok(Some(ChatStreamChunk {
            content,
            finish_reason,
            usage,
        }))
    }
}

fn saturating_token_u32(value: Option<&serde_json::Value>) -> u32 {
    value
        .and_then(|v| v.as_u64())
        .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
        .unwrap_or(0)
}

fn parse_usage(usage: Option<&serde_json::Value>) -> Option<Usage> {
    usage.as_ref()?.as_object()?;
    Some(Usage {
        prompt_tokens: saturating_token_u32(usage.and_then(|u| u.get("prompt_tokens"))),
        completion_tokens: saturating_token_u32(usage.and_then(|u| u.get("completion_tokens"))),
        total_tokens: saturating_token_u32(usage.and_then(|u| u.get("total_tokens"))),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamCheckpoint {
    pub generation: u64,
    pub request_id: String,
    pub model: String,
    pub chunks_received: u64,
    pub content_so_far: String,
    pub completion_tokens_accumulated: u64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_kind_as_label_matches_debug_format() {
        let variants = vec![
            StrategyKind::Single,
            StrategyKind::Consensus,
            StrategyKind::Reflection,
            StrategyKind::Chain,
            StrategyKind::Debate,
            StrategyKind::ReAct,
            StrategyKind::Fusion,
            StrategyKind::Custom("my_custom_strategy".to_string()),
        ];

        for variant in variants {
            let expected = format!("{:?}", variant);
            let actual = variant.as_label();
            assert_eq!(
                actual, expected,
                "as_label must match Debug format for Prometheus metric label continuity"
            );
        }
    }

    #[test]
    fn test_sse_final_chunk_with_empty_delta_yields_finish_and_usage() {
        // OpenAI's final chunk: empty delta object, finish_reason + usage.
        let data = r#"{"id":"x","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":12,"completion_tokens":34,"total_tokens":46}}"#;
        let chunk = ChatStreamChunk::from_sse_data(data)
            .unwrap()
            .expect("final chunk with usage must not be dropped");
        assert_eq!(chunk.finish_reason.as_deref(), Some("stop"));
        let usage = chunk.usage.expect("usage must be populated");
        assert_eq!(usage.prompt_tokens, 12);
        assert_eq!(usage.completion_tokens, 34);
        assert_eq!(usage.total_tokens, 46);
        assert_eq!(chunk.content, None);
    }

    #[test]
    fn test_sse_finish_reason_parsed_alongside_content() {
        // Some upstreams send content AND finish_reason in the same frame.
        let data = r#"{"choices":[{"delta":{"content":"bye"},"finish_reason":"stop"}]}"#;
        let chunk = ChatStreamChunk::from_sse_data(data)
            .unwrap()
            .expect("frame with content must parse");
        assert_eq!(chunk.content.as_deref(), Some("bye"));
        assert_eq!(chunk.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn test_sse_missing_content_treated_like_empty() {
        // delta without a content key at all behaves like an empty delta.
        let data = r#"{"choices":[{"delta":{"role":"assistant"},"finish_reason":"length"}]}"#;
        let chunk = ChatStreamChunk::from_sse_data(data)
            .unwrap()
            .expect("missing-content final frame must parse");
        assert_eq!(chunk.finish_reason.as_deref(), Some("length"));
        assert_eq!(chunk.content, None);
    }

    #[test]
    fn test_sse_huge_token_counts_saturate_not_wrap() {
        let data = r#"{"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":18446744073709551615,"completion_tokens":99999999999,"total_tokens":7}}"#;
        let chunk = ChatStreamChunk::from_sse_data(data)
            .unwrap()
            .expect("usage frame must parse");
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, u32::MAX, "must saturate, not wrap");
        assert_eq!(usage.completion_tokens, u32::MAX, "must saturate, not wrap");
        assert_eq!(usage.total_tokens, 7);
    }

    #[test]
    fn test_sse_keepalive_frames_return_none() {
        assert!(ChatStreamChunk::from_sse_data("").unwrap().is_none());
        assert!(ChatStreamChunk::from_sse_data("[DONE]").unwrap().is_none());
        assert!(
            ChatStreamChunk::from_sse_data(r#"{"choices":[{"delta":{}}]}"#)
                .unwrap()
                .is_none(),
            "pure keep-alive frame (empty delta, no finish/usage) carries no payload"
        );
        // Legacy shape preserved: empty-string content without finish/usage.
        assert!(
            ChatStreamChunk::from_sse_data(r#"{"choices":[{"delta":{"content":""}}]}"#)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_sse_content_only_frame_unchanged() {
        let data = r#"{"choices":[{"delta":{"content":"hello"}}]}"#;
        let chunk = ChatStreamChunk::from_sse_data(data)
            .unwrap()
            .expect("content frame must parse");
        assert_eq!(chunk.content.as_deref(), Some("hello"));
        assert_eq!(chunk.finish_reason, None);
        assert!(chunk.usage.is_none());
    }
}
