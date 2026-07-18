use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ExecutionIntent {
    Quality,
    Speed,
    Balanced,
    Exhaustive,
    Constrained {
        max_latency_ms: Option<u64>,
        max_cost_usd: Option<f64>,
        max_tokens: Option<u64>,
        min_confidence: Option<f32>,
    },
}

impl Default for ExecutionIntent {
    fn default() -> Self {
        Self::Balanced
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OutputPreferences {
    #[serde(default)]
    pub include_report: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub graph: GraphSummary,
    pub costs: Vec<ModelCost>,
    pub timing: TimingInfo,
    pub model_breakdown: HashMap<String, Usage>,
    pub decisions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSummary {
    pub node_count: usize,
    pub max_depth: usize,
    pub strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCost {
    pub model: String,
    pub cost: f64,
    pub tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingInfo {
    pub total_ms: u64,
    pub per_node: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}
