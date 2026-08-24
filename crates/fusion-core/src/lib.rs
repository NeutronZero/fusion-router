pub mod monetary;
pub use monetary::NanoUSD;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExecutionId(pub Uuid);

impl ExecutionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ExecutionId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkerId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PolicyId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StrategyId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionState {
    Created,
    Compiled,
    Scheduled,
    Queued,
    Running,
    Retrying,
    Completed,
    Failed,
    Cancelled,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionOutcome {
    pub execution_id: ExecutionId,
    pub status: ExecutionState,
    pub output_payload: serde_json::Value,
    pub total_cost: NanoUSD,
    pub total_duration_ms: u64,
    pub produced_artifacts: Vec<String>,
    pub completed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderLifecycleState {
    NotConfigured,
    Configured,
    Validated,
    Available,
    Healthy,
    Serving,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    Created,
    Queued,
    Running,
    Retrying,
    Completed,
    Failed,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlatformStatus {
    Starting,
    Initializing,
    LoadingPlugins,
    LoadingConfiguration,
    DiscoveringProviders,
    Ready,
    Draining,
    Stopping,
    Stopped,
}

#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum PlatformError {
    #[error("Compiler error [{code}]: {message} (Suggestion: {recovery_suggestion})")]
    Compiler {
        code: String,
        message: String,
        recovery_suggestion: String,
    },
    #[error("Planner error [{code}]: {message} (Suggestion: {recovery_suggestion})")]
    Planner {
        code: String,
        message: String,
        recovery_suggestion: String,
    },
    #[error("Runtime error [{code}]: {message} (Suggestion: {recovery_suggestion})")]
    Runtime {
        code: String,
        message: String,
        recovery_suggestion: String,
    },
    #[error("Scheduler error [{code}]: {message} (Suggestion: {recovery_suggestion})")]
    Scheduler {
        code: String,
        message: String,
        recovery_suggestion: String,
    },
    #[error("Plugin error [{code}]: {message} (Suggestion: {recovery_suggestion})")]
    Plugin {
        code: String,
        message: String,
        recovery_suggestion: String,
    },
    #[error("Storage error [{code}]: {message} (Suggestion: {recovery_suggestion})")]
    Storage {
        code: String,
        message: String,
        recovery_suggestion: String,
    },
    #[error("Security error [{code}]: {message} (Suggestion: {recovery_suggestion})")]
    Security {
        code: String,
        message: String,
        recovery_suggestion: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
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
