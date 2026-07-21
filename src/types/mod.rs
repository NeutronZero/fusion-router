use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub mod execution;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionGraph {
    pub graph_id: Uuid,
    pub nodes: Vec<ExecutionNode>,
    pub edges: Vec<ExecutionEdge>,
    pub metadata: GraphMetadata,
    pub total_tokens: u64,
    pub total_cost: u64,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionNodeKind {
    LLMGenerate,
    LLMReview,
    LLMJudge,
    Transform,
    Gate,
    Aggregate,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionInstance {
    pub instance_id: Uuid,
    pub graph: ExecutionGraph,
    pub node_states: HashMap<Uuid, NodeState>,
    pub outputs: HashMap<Uuid, serde_json::Value>,
    pub reservation_id: Uuid,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeState {
    Pending,
    Running,
    Succeeded,
    Failed(String),
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub instance_id: Uuid,
    pub success: bool,
    pub outputs: HashMap<Uuid, serde_json::Value>,
    pub total_latency_ms: u64,
    pub total_cost: f64,
    pub total_tokens: u64,
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
            code: "claude-sonnet-4-20250514".into(),
            debug: "claude-sonnet-4-20250514".into(),
            architecture: "claude-opus-4-20250514".into(),
            general: "gpt-4o".into(),
            creative: "claude-sonnet-4-20250514".into(),
            analysis: "claude-opus-4-20250514".into(),
            fast: "gpt-4o-mini".into(),
            cheap: "gpt-4o-mini".into(),
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
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeExecutionResult {
    pub state: NodeState,
    pub usage: Option<Usage>,
    pub latency_ms: u64,
    pub output: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
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
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
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
    #[error("Internal compiler error: {0}")]
    Internal(String),
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
