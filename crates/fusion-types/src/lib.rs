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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub use fusion_core::NanoUSD;

pub mod execution;

pub use execution::{ExecutionIntent, OutputPreferences};

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
    #[serde(default)]
    pub policy_version: u64,
    pub estimated_cost: NanoUSD,
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
    pub total_cost: NanoUSD,
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
    pub estimated_cost: NanoUSD,
    pub estimated_tokens: u64,
    #[serde(default)]
    pub policy_version: u64,
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
    pub total_cost: NanoUSD,
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
    pub cost: NanoUSD,
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

/// Canonical `ModelCatalog` lives in `fusion-core` and is re-exported here so
/// the execution layer has a single definition (previously two independent,
/// drift-prone copies).
pub use fusion_core::ModelCatalog;

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
    pub avg_costs: HashMap<String, NanoUSD>,
    pub model_rankings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quota {
    pub max_daily_cost: NanoUSD,
    pub max_daily_tokens: u64,
    pub max_concurrent: u32,
    pub provider_limits: HashMap<String, ProviderLimit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderLimit {
    pub max_daily_cost: NanoUSD,
    pub max_rpm: u32,
    pub max_tpm: u64,
}

// ---------------------------------------------------------------------------
// Request budget envelope
// ---------------------------------------------------------------------------

/// Per-request spending guard shared across a run (Phase 6.3b: lifted from
/// `src/resource/budget.rs` so `fusion_scheduler` can enforce it inline).
/// All clones share the same counters, so the pipeline can hand one envelope
/// to multiple stages and observe accumulated spend.
#[derive(Debug)]
pub struct BudgetEnvelope {
    pub max_cost: NanoUSD,
    pub max_tokens: u64,
    pub max_iterations: u32,
    spent_cost: Arc<AtomicU64>,
    spent_tokens: Arc<AtomicU64>,
    current_iterations: Arc<AtomicU64>,
}

impl BudgetEnvelope {
    pub fn new(max_cost: NanoUSD, max_tokens: u64, max_iterations: u32) -> Self {
        Self {
            max_cost,
            max_tokens,
            max_iterations,
            spent_cost: Arc::new(AtomicU64::new(0)),
            spent_tokens: Arc::new(AtomicU64::new(0)),
            current_iterations: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Records spend and enforces both budget ceilings.
    ///
    /// Ordering (review finding S6): the check happens BEFORE the spend is
    /// committed. On violation, both counters are rolled back to their prior
    /// values so a rejected record leaves no permanent overspend. The error
    /// still reports the prospective total for diagnostics. Under concurrent
    /// records a transient over-read is possible (another thread may observe
    /// the reservation before rollback) â€” that fails closed rather than open.
    ///
    /// Arithmetic is checked: a record whose addition would overflow a u64
    /// counter wraps nothing â€” nothing is committed for the overflowing
    /// counter and a saturated ceiling-violation error is returned instead
    /// (plain adds wrap in release builds, which previously let a huge
    /// reservation bypass the ceilings).
    pub fn record_and_check(&self, cost: NanoUSD, tokens: u64) -> Result<(), BudgetExceededError> {
        let cost_nanos = cost.as_nanos();

        // `fetch_update` yields the PREVIOUS counter value; the committed new
        // total is prev + increment, which the closure already proved fits.
        let prev_cost =
            match self
                .spent_cost
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |prev| {
                    prev.checked_add(cost_nanos)
                }) {
                Ok(prev) => prev,
                Err(_overflow) => {
                    return Err(BudgetExceededError::Cost {
                        spent: u64::MAX,
                        max: self.max_cost.as_nanos(),
                    });
                }
            };
        let new_cost = NanoUSD::from_nanos(prev_cost.saturating_add(cost_nanos));

        let prev_tokens =
            match self
                .spent_tokens
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |prev| {
                    prev.checked_add(tokens)
                }) {
                Ok(prev) => prev,
                Err(_overflow) => {
                    self.spent_cost.fetch_sub(cost_nanos, Ordering::SeqCst);
                    return Err(BudgetExceededError::Tokens {
                        spent: u64::MAX,
                        max: self.max_tokens,
                    });
                }
            };
        let new_tokens = prev_tokens.saturating_add(tokens);

        let violation = if new_cost > self.max_cost {
            Some(BudgetExceededError::Cost {
                spent: new_cost.as_nanos(),
                max: self.max_cost.as_nanos(),
            })
        } else if new_tokens > self.max_tokens {
            Some(BudgetExceededError::Tokens {
                spent: new_tokens,
                max: self.max_tokens,
            })
        } else {
            None
        };

        if let Some(err) = violation {
            self.spent_cost.fetch_sub(cost_nanos, Ordering::SeqCst);
            self.spent_tokens.fetch_sub(tokens, Ordering::SeqCst);
            return Err(err);
        }
        Ok(())
    }

    pub fn increment_iteration(&self) -> Result<u64, BudgetExceededError> {
        let iter = self.current_iterations.fetch_add(1, Ordering::SeqCst) + 1;
        if iter > self.max_iterations as u64 {
            self.current_iterations.fetch_sub(1, Ordering::SeqCst);
            return Err(BudgetExceededError::Iterations {
                current: iter,
                max: self.max_iterations,
            });
        }
        Ok(iter)
    }

    pub fn spent_cost(&self) -> NanoUSD {
        NanoUSD::from_nanos(self.spent_cost.load(Ordering::Acquire))
    }

    pub fn spent_tokens(&self) -> u64 {
        self.spent_tokens.load(Ordering::Acquire)
    }

    pub fn current_iterations(&self) -> u64 {
        self.current_iterations.load(Ordering::Acquire)
    }
}

impl Clone for BudgetEnvelope {
    fn clone(&self) -> Self {
        Self {
            max_cost: self.max_cost,
            max_tokens: self.max_tokens,
            max_iterations: self.max_iterations,
            spent_cost: self.spent_cost.clone(),
            spent_tokens: self.spent_tokens.clone(),
            current_iterations: self.current_iterations.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BudgetExceededError {
    Cost { spent: u64, max: u64 },
    Tokens { spent: u64, max: u64 },
    Iterations { current: u64, max: u32 },
}

impl std::fmt::Display for BudgetExceededError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cost { spent, max } => write!(
                f,
                "Cost budget exceeded: {} nano-USD spent, {} nano-USD max",
                spent, max
            ),
            Self::Tokens { spent, max } => write!(
                f,
                "Token budget exceeded: {} tokens spent, {} max",
                spent, max
            ),
            Self::Iterations { current, max } => write!(
                f,
                "Iteration budget exceeded: {} iterations, {} max",
                current, max
            ),
        }
    }
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
    PassError { pass: String, message: String },
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
            assert_eq!(
                actual, expected,
                "as_label must match Debug format for Prometheus metric label continuity"
            );
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
            total_cost: NanoUSD::from_nanos(50_000_000),
            total_tokens: 500,
            terminal_node_id: None,
            final_output: None,
        };
        let cloned = result.clone();
        assert_eq!(result.instance_id, cloned.instance_id);
        assert_eq!(result.success, cloned.success);
    }

    #[test]
    fn test_budget_record_within_and_beyond_limits() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 100, 5);
        assert!(env.record_and_check(NanoUSD::from_nanos(500), 30).is_ok());
        assert_eq!(env.spent_cost().as_nanos(), 500);
        assert_eq!(env.spent_tokens(), 30);
        let err = env
            .record_and_check(NanoUSD::from_nanos(600), 30)
            .unwrap_err();
        assert_eq!(
            err,
            BudgetExceededError::Cost {
                spent: 1100,
                max: 1000
            }
        );
    }

    #[test]
    fn test_budget_iteration_cap() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 100, 2);
        assert!(env.increment_iteration().is_ok());
        assert!(env.increment_iteration().is_ok());
        let err = env.increment_iteration().unwrap_err();
        assert_eq!(err, BudgetExceededError::Iterations { current: 3, max: 2 });
    }

    #[test]
    fn test_budget_clone_shares_atomics() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 100, 5);
        let cloned = env.clone();
        assert!(env.record_and_check(NanoUSD::from_nanos(300), 50).is_ok());
        assert_eq!(cloned.spent_cost().as_nanos(), 300);
        assert_eq!(cloned.spent_tokens(), 50);
    }

    #[test]
    fn test_budget_failure_rolls_back_spend() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(100), 50, 5);
        assert!(env.record_and_check(NanoUSD::from_nanos(60), 30).is_ok());
        let err = env.record_and_check(NanoUSD::from_nanos(60), 30);
        assert!(err.is_err());
        assert_eq!(
            err.unwrap_err(),
            BudgetExceededError::Cost {
                spent: 120,
                max: 100
            }
        );
        assert_eq!(env.spent_cost().as_nanos(), 60);
        assert_eq!(env.spent_tokens(), 30);
        let ok = env.record_and_check(NanoUSD::from_nanos(40), 20);
        assert!(ok.is_ok(), "rolled-back spend must free budget again");
        assert_eq!(env.spent_cost().as_nanos(), 100);
        assert_eq!(env.spent_tokens(), 50);
    }

    #[test]
    fn test_budget_cost_overflow_rejected_without_committing() {
        let half = u64::MAX / 2 + 1;
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(u64::MAX), u64::MAX, 5);
        assert!(env.record_and_check(NanoUSD::from_nanos(half), 0).is_ok());
        // half + half would exceed u64::MAX: must fail closed, not wrap.
        let err = env
            .record_and_check(NanoUSD::from_nanos(half), 0)
            .unwrap_err();
        assert_eq!(
            err,
            BudgetExceededError::Cost {
                spent: u64::MAX,
                max: u64::MAX
            }
        );
        assert_eq!(
            env.spent_cost().as_nanos(),
            half,
            "overflow must not commit"
        );
        assert_eq!(env.spent_tokens(), 0);
        // Envelope remains usable for spend within the remaining headroom.
        assert!(env
            .record_and_check(NanoUSD::from_nanos(u64::MAX - half), 0)
            .is_ok());
        assert_eq!(env.spent_cost().as_nanos(), u64::MAX);
    }

    #[test]
    fn test_budget_token_overflow_rolls_back_cost() {
        let half = u64::MAX / 2 + 1;
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(u64::MAX), u64::MAX, 5);
        assert!(env.record_and_check(NanoUSD::from_nanos(10), half).is_ok());
        let err = env
            .record_and_check(NanoUSD::from_nanos(10), half)
            .unwrap_err();
        assert_eq!(
            err,
            BudgetExceededError::Tokens {
                spent: u64::MAX,
                max: u64::MAX
            }
        );
        assert_eq!(env.spent_tokens(), half);
        assert_eq!(
            env.spent_cost().as_nanos(),
            10,
            "cost must roll back with tokens"
        );
    }

    #[test]
    fn test_budget_iteration_failure_rolls_back_counter() {
        let env = BudgetEnvelope::new(NanoUSD::from_nanos(1000), 100, 2);
        assert_eq!(env.increment_iteration().unwrap(), 1);
        assert_eq!(env.increment_iteration().unwrap(), 2);
        let err = env.increment_iteration().unwrap_err();
        assert_eq!(err, BudgetExceededError::Iterations { current: 3, max: 2 });
        assert_eq!(
            env.current_iterations(),
            2,
            "failed iterations must consume nothing"
        );
        // Repeated failures keep reporting the same prospective count.
        let again = env.increment_iteration().unwrap_err();
        assert_eq!(
            again,
            BudgetExceededError::Iterations { current: 3, max: 2 }
        );
        assert_eq!(env.current_iterations(), 2);
    }

    #[test]
    fn test_budget_error_display_reports_nano_usd_units() {
        let err = BudgetExceededError::Cost {
            spent: 123,
            max: 100,
        };
        let text = err.to_string();
        assert!(text.contains("nano-USD"), "unit must be nano-USD: {text}");
        assert!(!text.contains("millicosts"), "stale unit label: {text}");
    }
}
