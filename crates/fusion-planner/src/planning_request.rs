use fusion_core::{ModelCatalog, NanoUSD};
use fusion_kernel::CapabilityCatalog;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExecutionIntent {
    Quality,
    Speed,
    Balanced,
    Exhaustive,
    Constrained { max_cost: Option<NanoUSD> },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PolicyDeclarationSnapshot {
    pub id: String,
    pub name: String,
    pub rule: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PolicySnapshot {
    pub version: u64,
    pub policies: Vec<PolicyDeclarationSnapshot>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoutingTelemetrySnapshot {
    pub avg_latency_ms: u64,
    pub error_rate: f64,
    pub healthy_provider_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RequirementsSnapshot {
    pub complexity: String,
    pub execution_intent: Option<ExecutionIntent>,
    pub required_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningRequest {
    pub intent: ExecutionIntent,
    pub user_prompt: String,
    pub requested_model: Option<String>,
    pub requirements: RequirementsSnapshot,
    pub policies: PolicySnapshot,
    pub capability_catalog: CapabilityCatalog,
    pub model_catalog: ModelCatalog,
    pub telemetry: RoutingTelemetrySnapshot,
}
