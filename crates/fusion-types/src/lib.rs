//! # fusion-types
//!
//! Execution-layer types shared across the fusion-router workspace.
//!
//! This crate defines the canonical execution-plane IR (`WorkflowIR`, `ExecutionGraph`,
//! node/edge types, error types, and supporting value objects). The planning-level
//! IR lives in `fusion-ir` (provider-free, immutable). An adapter bridges the two
//! at the `src/ir/adapter.rs` boundary.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod execution;

pub use execution::{
    ExecutionIntent, OutputPreferences,
};

// ---------------------------------------------------------------------------
// IR types (planner output / compiler input)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowIR {
    pub plan_id: uuid::Uuid,
    pub nodes: Vec<IRNode>,
    pub edges: Vec<IREdge>,
    pub metadata: IRMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IRNode {
    pub id: uuid::Uuid,
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
    pub from: uuid::Uuid,
    pub to: uuid::Uuid,
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IRMetadata {
    pub policy_applied: Vec<String>,
    pub estimated_cost: f64,
    pub estimated_tokens: u64,
}

// ---------------------------------------------------------------------------
// Strategy
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Execution graph (compiler output / scheduler input)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionGraph {
    pub graph_id: uuid::Uuid,
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
    pub id: uuid::Uuid,
    pub kind: ExecutionNodeKind,
    pub strategy: StrategyKind,
    pub model: String,
    pub retry_policy: RetryPolicy,
    pub fallback: Option<FallbackConfig>,
    pub config: HashMap<String, serde_json::Value>,
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
    pub from: uuid::Uuid,
    pub to: uuid::Uuid,
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
pub struct ExecutionSubgraph {
    pub nodes: Vec<ExecutionNode>,
    pub edges: Vec<ExecutionEdge>,
    pub entry_node_id: uuid::Uuid,
    pub exit_node_id: uuid::Uuid,
}

// ---------------------------------------------------------------------------
// Runtime state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeState {
    Pending,
    Running,
    Succeeded,
    Failed(String),
    Skipped,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub instance_id: uuid::Uuid,
    pub success: bool,
    pub outputs: HashMap<uuid::Uuid, serde_json::Value>,
    pub total_latency_ms: u64,
    pub total_cost: f64,
    pub total_tokens: u64,
    pub terminal_node_id: Option<uuid::Uuid>,
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
pub struct ReservationId(pub uuid::Uuid);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeExecutionResult {
    pub state: NodeState,
    pub usage: Option<Usage>,
    pub latency_ms: u64,
    pub output: Option<serde_json::Value>,
}

/// Context passed to an executor for a single node execution.
///
/// `parent_outputs` maps predecessor node IDs to their outputs (immediate
/// dependencies only). `graph_outputs` contains all outputs produced so far.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeExecContext {
    pub parent_outputs: HashMap<uuid::Uuid, serde_json::Value>,
    pub graph_outputs: HashMap<uuid::Uuid, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub record_id: uuid::Uuid,
    pub plan_id: uuid::Uuid,
    pub node_id: uuid::Uuid,
    pub model: String,
    pub provider: String,
    pub intent: Intent,
    pub latency_ms: u64,
    pub tokens: u32,
    pub cost: f64,
    pub success: bool,
    pub timestamp: i64,
}

// ---------------------------------------------------------------------------
// Shared value objects
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
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
    // NOTE: model_requirements stays in src/types/ until providers are extracted
    // (it references crate::providers::ModelRequirements)
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

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum CompilerError {
    #[error("Validation error in pass '{pass}': {message}")]
    ValidationError {
        pass: String,
        node_id: Option<uuid::Uuid>,
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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

    #[test]
    fn test_model_catalog_default() {
        let catalog = ModelCatalog::default();
        assert!(catalog.code.is_empty());
        assert!(catalog.fast.is_empty());
    }

    #[test]
    fn test_execution_result_clone() {
        let result = ExecutionResult {
            instance_id: uuid::Uuid::new_v4(),
            success: true,
            outputs: HashMap::new(),
            total_latency_ms: 100,
            total_cost: 0.05,
            total_tokens: 500,
            terminal_node_id: None,
            final_output: None,
        };
        let cloned = result.clone();
        assert_eq!(result.instance_id, cloned.instance_id);
        assert_eq!(result.success, cloned.success);
    }
}
