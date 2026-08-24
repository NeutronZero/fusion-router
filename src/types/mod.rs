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
        if content.as_deref() == Some("") {
            let finish = body["choices"][0]["finish_reason"]
                .as_str()
                .map(|s| s.to_string());
            let usage = body["usage"].as_object().map(|u| Usage {
                prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
            });
            if finish.is_some() || usage.is_some() {
                return Ok(Some(ChatStreamChunk {
                    content: None,
                    finish_reason: finish,
                    usage,
                }));
            }
            return Ok(None);
        }
        Ok(Some(ChatStreamChunk {
            content,
            finish_reason: None,
            usage: None,
        }))
    }
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
}
