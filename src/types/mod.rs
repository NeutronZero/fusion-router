use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub mod error;
pub mod execution;
pub mod artifact;
pub mod execution_context;
pub mod anthropic;

pub use error::{PipelineStage, RouterError};
pub use artifact::ArtifactKind;
pub use anthropic::{AnthropicMessagesRequest, AnthropicMessagesResponse};

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
    pub execution: Option<execution::ExecutionIntent>,
    #[serde(default)]
    pub output: Option<execution::OutputPreferences>,
    /// Optional strategy override. When set, the planned workflow is replaced
    /// by a single node executing the named strategy — e.g. a multi-model
    /// consensus that fans out to `count` reviewers, each on its own
    /// `members[i]` model, with a judge consolidating the reviews.
    #[serde(default)]
    pub strategy: Option<RequestStrategy>,
}

/// Ensemble strategy declared at the request level.
///
/// Example — have three different models review the code and a judge merge
/// their reports:
///
/// ```json
/// "strategy": {
///   "kind": "Consensus",
///   "count": 3,
///   "members": ["zen/deepseek-v4-flash-free", "openrouter/moonshotai/kimi-k3-free", "openrouter/deepseek/deepseek-r1-0528:free"],
///   "max_tool_rounds": 8
/// }
/// ```
///
/// `members` per model can come from any registered provider (routing is by
/// model key prefix); members beyond the list length cycle. `kind` supports
/// the built-in strategy kinds when lowerable; `max_tool_rounds` bounds the
/// tool loop for every member.
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
pub struct ChatMessage {
    pub role: String,
    pub content: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirements {
    pub intent_classification: Intent,
    pub complexity: ComplexityLevel,
    pub has_files: bool,
    pub context_window: u64,
    pub original_text: String,
    #[serde(default)]
    pub execution_intent: Option<execution::ExecutionIntent>,
    #[serde(default)]
    pub output_preferences: Option<execution::OutputPreferences>,
    #[serde(default, skip)]
    pub model_requirements: Option<crate::providers::ModelRequirements>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Intent {
    Code,
    Debug,
    Architecture,
    General,
    Creative,
    Analysis,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ComplexityLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowIR {
    pub plan_id: Uuid,
    pub nodes: Vec<IRNode>,
    pub edges: Vec<IREdge>,
    pub metadata: IRMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IRNode {
    pub id: Uuid,
    pub kind: IRNodeKind,
    pub strategy: StrategyKind,
    pub model: Option<String>,
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IRNodeKind {
    Generate,
    Review,
    Judge,
    Transform,
    Gate,
    Conditional,
    Loop,
    Split,
    Join,
    Barrier,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IREdge {
    pub from: Uuid,
    pub to: Uuid,
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IRMetadata {
    pub policy_applied: Vec<String>,
    pub estimated_cost: f64,
    pub estimated_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum StrategyKind {
    Single,
    Consensus,
    Reflection,
    Chain,
    Debate,
    ReAct,
    Fusion,
    Custom(String),
}

impl StrategyKind {
    pub fn as_label(&self) -> std::borrow::Cow<'static, str> {
        match self {
            StrategyKind::Single => std::borrow::Cow::Borrowed("Single"),
            StrategyKind::Consensus => std::borrow::Cow::Borrowed("Consensus"),
            StrategyKind::Reflection => std::borrow::Cow::Borrowed("Reflection"),
            StrategyKind::Chain => std::borrow::Cow::Borrowed("Chain"),
            StrategyKind::Debate => std::borrow::Cow::Borrowed("Debate"),
            StrategyKind::ReAct => std::borrow::Cow::Borrowed("ReAct"),
            StrategyKind::Fusion => std::borrow::Cow::Borrowed("Fusion"),
            StrategyKind::Custom(name) => std::borrow::Cow::Owned(format!("Custom({name:?})")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionGraph {
    pub graph_id: Uuid,
    pub nodes: Vec<ExecutionNode>,
    pub edges: Vec<ExecutionEdge>,
    pub metadata: GraphMetadata,
    pub total_tokens: u64,
    pub total_cost: u64,
    #[serde(default)]
    pub primitive_graph_hash: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionNode {
    pub id: Uuid,
    pub kind: ExecutionNodeKind,
    pub strategy: StrategyKind,
    pub model: String,
    pub retry_policy: RetryPolicy,
    pub fallback: Option<FallbackConfig>,
    pub config: HashMap<String, serde_json::Value>,
    /// Pre-lowered strategy subgraph, attached at compile time by the
    /// strategy expansion in `lower_to_graph`. `None` for passthrough
    /// (Single strategy, unexpanded legacy graphs, or strategy lowering
    /// fallback). When `Some`, the executor executes this subgraph directly
    /// instead of lowering the strategy at runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subgraph: Option<ExecutionSubgraph>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExecutionNodeKind {
    LLMGenerate,
    LLMReview,
    LLMJudge,
    Transform,
    Gate,
    Conditional,
    Loop,
    Split,
    Join,
    Barrier,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEdge {
    pub from: Uuid,
    pub to: Uuid,
    #[serde(default)]
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphMetadata {
    pub estimated_cost: f64,
    pub estimated_tokens: u64,
    pub max_depth: u32,
    pub node_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub backoff_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackConfig {
    pub model: String,
    pub provider: String,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeState {
    Pending,
    Running,
    Succeeded,
    Failed(String),
    Skipped,
}

/// Runtime ABI contract between Scheduler and Pipeline.
/// Changes to this structure impact response building, telemetry, and execution reporting.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub instance_id: Uuid,
    pub success: bool,
    pub outputs: HashMap<Uuid, serde_json::Value>,
    pub total_latency_ms: u64,
    pub total_cost: f64,
    pub total_tokens: u64,
    pub terminal_node_id: Option<Uuid>,
    pub final_output: Option<serde_json::Value>,
}

impl Clone for ExecutionResult {
    fn clone(&self) -> Self {
        Self {
            instance_id: self.instance_id,
            success: self.success,
            outputs: self.outputs.clone(),
            total_latency_ms: self.total_latency_ms,
            total_cost: self.total_cost,
            total_tokens: self.total_tokens,
            terminal_node_id: self.terminal_node_id,
            final_output: self.final_output.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub name: String,
    pub priority: u32,
    pub conditions: Vec<PolicyCondition>,
    pub actions: Vec<PolicyAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyCondition {
    pub field: String,
    pub operator: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyAction {
    pub action_type: String,
    pub params: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalog {
    pub code: String,
    pub debug: String,
    pub architecture: String,
    pub general: String,
    pub creative: String,
    pub analysis: String,
    pub fast: String,
    pub cheap: String,
}

impl Default for ModelCatalog {
    fn default() -> Self {
        Self {
            code: String::new(),
            debug: String::new(),
            architecture: String::new(),
            general: String::new(),
            creative: String::new(),
            analysis: String::new(),
            fast: String::new(),
            cheap: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReservationId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSubgraph {
    pub nodes: Vec<ExecutionNode>,
    pub edges: Vec<ExecutionEdge>,
    pub entry_node_id: Uuid,
    pub exit_node_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeExecutionResult {
    pub state: NodeState,
    pub usage: Option<Usage>,
    pub latency_ms: u64,
    pub output: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub record_id: Uuid,
    pub plan_id: Uuid,
    pub node_id: Uuid,
    pub model: String,
    pub provider: String,
    pub intent: Intent,
    pub latency_ms: u64,
    pub tokens: u32,
    pub cost: f64,
    pub success: bool,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceSnapshot {
    pub record_count: u64,
    pub success_rates: HashMap<String, f64>,
    pub avg_latencies: HashMap<String, f64>,
    pub avg_costs: HashMap<String, f64>,
    pub model_rankings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quota {
    pub max_daily_cost: f64,
    pub max_daily_tokens: u64,
    pub max_concurrent: u32,
    pub provider_limits: HashMap<String, ProviderLimit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderLimit {
    pub max_daily_cost: f64,
    pub max_rpm: u32,
    pub max_tpm: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Runtime ABI contract between providers and executor.
/// Tool execution is fed ONLY from provider-native `tool_calls`
/// (Law 7 / ADR-037); model output text is never parsed for tool calls.
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
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
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
        let content = body["choices"][0]["delta"]["content"].as_str().map(|s| s.to_string());
        if content.as_deref() == Some("") {
            let finish = body["choices"][0]["finish_reason"].as_str().map(|s| s.to_string());
            let usage = body["usage"].as_object().map(|u| Usage {
                prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
            });
            if finish.is_some() || usage.is_some() {
                return Ok(Some(ChatStreamChunk { content: None, finish_reason: finish, usage }));
            }
            return Ok(None);
        }
        Ok(Some(ChatStreamChunk { content, finish_reason: None, usage: None }))
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

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum CompilerError {
    #[error("Validation error in pass '{pass}': {message}")]
    ValidationError {
        pass: String,
        node_id: Option<Uuid>,
        message: String,
    },
    #[error("Pass '{pass}' failed: {message}")]
    PassError {
        pass: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum SchedulerError {
    #[error("Node execution failed: {0}")]
    NodeFailed(String),
    #[error("Cyclic dependency detected")]
    CyclicDependency,
    #[error("Internal error: {0}")]
    Internal(String),
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
            assert_eq!(actual, expected, "as_label must match Debug format for Prometheus metric label continuity");
        }
    }
}
