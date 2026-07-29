//! `fusion-plugin-api`
//!
//! Minimal, lightweight public SDK for building FusionRouter plugins and capabilities.

/// Current ABI version for capability packages (ADR-018).
pub const CAPABILITY_ABI_VERSION: &str = "0.1.0";

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Strongly-typed identifier for a capability (e.g., `echo.text`, `github.issue.create`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CapabilityId(pub String);

impl CapabilityId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Metadata declared by a plugin for version compatibility checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub name: String,
    pub version: semver::Version,
    pub api_version: semver::Version,
    pub min_compiler_version: semver::Version,
    pub capabilities: Vec<CapabilityId>,
}

/// Declarative ABI contract exposed by a capability to the Planner & Scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityContract {
    pub id: CapabilityId,
    pub version: semver::Version,
    pub description: String,
    pub inputs_schema: serde_json::Value,
    pub outputs_schema: serde_json::Value,
    pub permissions: Vec<String>,
    pub estimated_cost_usd: f64,
    pub estimated_latency_ms: u64,
    pub reliability_score: f32,
    pub supports_streaming: bool,
}

/// Bound runtime execution object pairing a `CapabilityContract` with runtime execution contexts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityInstance {
    pub contract: CapabilityContract,
    pub runtime_params: serde_json::Value,
}

/// Standardized output struct returned by plugin capability execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub outputs: serde_json::Value,
    pub metrics: std::collections::HashMap<String, f64>,
}

/// Structured error classification for plugin execution failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionError {
    pub connector: String,
    pub capability: CapabilityId,
    pub reason: String,
    pub retryable: bool,
}

/// Core trait implemented by all plugins.
pub trait Plugin: Send + Sync {
    fn metadata(&self) -> PluginMetadata;
}

/// Trait implemented by plugins that expose capabilities.
pub trait CapabilityPlugin: Plugin {
    fn capabilities(&self) -> Vec<CapabilityContract>;
}

/// Trait implemented by runtime executors for capability invocation.
#[async_trait]
pub trait CapabilityExecutor: Send + Sync {
    async fn execute(
        &self,
        instance: &CapabilityInstance,
        input: serde_json::Value,
    ) -> Result<ExecutionResult, ExecutionError>;
}
