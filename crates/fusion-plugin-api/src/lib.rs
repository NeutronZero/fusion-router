//! `fusion-plugin-api`
//!
//! Minimal, lightweight public SDK for building FusionRouter plugins and capabilities.

/// Current ABI version for capability packages (ADR-018).
pub const CAPABILITY_ABI_VERSION: &str = "0.2.0";

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

/// Error type for `Permission::validate()`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PermissionError {
    #[error("permission argument must not be empty")]
    EmptyArgument,
}

/// Typed permission model for capability ABI contracts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Permission {
    Network,
    Filesystem(String),
    Http(String),
    Secrets(String),
    Environment(String),
}

impl Permission {
    /// Validates that parameterized permissions have non-empty arguments.
    pub fn validate(&self) -> Result<(), PermissionError> {
        match self {
            Permission::Network => Ok(()),
            Permission::Filesystem(path) if path.is_empty() => Err(PermissionError::EmptyArgument),
            Permission::Http(endpoint) if endpoint.is_empty() => Err(PermissionError::EmptyArgument),
            Permission::Secrets(name) if name.is_empty() => Err(PermissionError::EmptyArgument),
            Permission::Environment(name) if name.is_empty() => Err(PermissionError::EmptyArgument),
            _ => Ok(()),
        }
    }
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Permission::Network => write!(f, "Network"),
            Permission::Filesystem(path) => write!(f, "Filesystem({path})"),
            Permission::Http(endpoint) => write!(f, "Http({endpoint})"),
            Permission::Secrets(name) => write!(f, "Secrets({name})"),
            Permission::Environment(name) => write!(f, "Environment({name})"),
        }
    }
}

impl std::str::FromStr for Permission {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(arg) = s.strip_prefix("Filesystem(").and_then(|s| s.strip_suffix(')')) {
            return Ok(Permission::Filesystem(arg.to_string()));
        }
        if let Some(arg) = s.strip_prefix("Http(").and_then(|s| s.strip_suffix(')')) {
            return Ok(Permission::Http(arg.to_string()));
        }
        if let Some(arg) = s.strip_prefix("Secrets(").and_then(|s| s.strip_suffix(')')) {
            return Ok(Permission::Secrets(arg.to_string()));
        }
        if let Some(arg) = s.strip_prefix("Environment(").and_then(|s| s.strip_suffix(')')) {
            return Ok(Permission::Environment(arg.to_string()));
        }
        if s == "Network" {
            return Ok(Permission::Network);
        }
        Err(format!("unknown permission variant: {s}"))
    }
}

/// Semantic execution traits advertised by capabilities (v0.13 contract 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityTrait {
    Streaming,
    LongContext,
    StructuredOutput,
    LowLatency,
    DeterministicOutput,
    ComputerUse,
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
    pub permissions: Vec<Permission>,
    pub dependencies: Vec<CapabilityId>,
    pub estimated_cost_usd: f64,
    pub estimated_latency_ms: u64,
    pub reliability_score: f32,
    pub supports_streaming: bool,
    #[serde(default)]
    pub traits: Vec<CapabilityTrait>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn permission_network_display() {
        assert_eq!(Permission::Network.to_string(), "Network");
    }

    #[test]
    fn permission_filesystem_display() {
        let p = Permission::Filesystem("/tmp".into());
        assert_eq!(p.to_string(), "Filesystem(/tmp)");
    }

    #[test]
    fn permission_http_display() {
        let p = Permission::Http("https://api.example.com".into());
        assert_eq!(p.to_string(), "Http(https://api.example.com)");
    }

    #[test]
    fn permission_from_str_network() {
        let p = Permission::from_str("Network").unwrap();
        assert_eq!(p, Permission::Network);
    }

    #[test]
    fn permission_from_str_filesystem() {
        let p = Permission::from_str("Filesystem(/tmp)").unwrap();
        assert_eq!(p, Permission::Filesystem("/tmp".into()));
    }

    #[test]
    fn permission_round_trips() {
        let cases = vec![
            Permission::Network,
            Permission::Filesystem("/data".into()),
            Permission::Http("https://example.com".into()),
            Permission::Secrets("API_KEY".into()),
            Permission::Environment("HOME".into()),
        ];
        for p in cases {
            let s = p.to_string();
            let back = Permission::from_str(&s).unwrap();
            assert_eq!(p, back, "round-trip failed for {s}");
        }
    }

    #[test]
    fn permission_validate_network_ok() {
        assert!(Permission::Network.validate().is_ok());
    }

    #[test]
    fn permission_validate_empty_filesystem_fails() {
        let p = Permission::Filesystem("".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn permission_validate_empty_http_fails() {
        let p = Permission::Http("".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn permission_validate_empty_secrets_fails() {
        let p = Permission::Secrets("".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn permission_validate_empty_environment_fails() {
        let p = Permission::Environment("".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn permission_json_round_trip() {
        let p = Permission::Filesystem("/tmp".into());
        let json = serde_json::to_string(&p).unwrap();
        let back: Permission = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn contract_permissions_typed() {
        let contract = CapabilityContract {
            id: CapabilityId::new("test.typed"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "typed permissions".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![Permission::Network, Permission::Http("https://example.com".into())],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        };
        assert_eq!(contract.permissions.len(), 2);
        assert!(matches!(contract.permissions[0], Permission::Network));
    }

    #[test]
    fn capability_trait_serde_round_trip() {
        for trait_ in [
            CapabilityTrait::Streaming,
            CapabilityTrait::LongContext,
            CapabilityTrait::StructuredOutput,
            CapabilityTrait::LowLatency,
            CapabilityTrait::DeterministicOutput,
            CapabilityTrait::ComputerUse,
        ] {
            let json = serde_json::to_string(&trait_).unwrap();
            let back: CapabilityTrait = serde_json::from_str(&json).unwrap();
            assert_eq!(back, trait_);
        }
    }

    #[test]
    fn contract_defaults_traits_to_empty() {
        let json = r#"{"id":"x.cap","version":"1.0.0","description":"d","inputs_schema":{},"outputs_schema":{},"permissions":[],"dependencies":[],"estimated_cost_usd":0.0,"estimated_latency_ms":0,"reliability_score":1.0,"supports_streaming":false}"#;
        let contract: CapabilityContract = serde_json::from_str(json).unwrap();
        assert!(contract.traits.is_empty());
    }
}
