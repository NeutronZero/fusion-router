//! Phase 7B — `ShellConnector` (`src/connectors/shell.rs`)

use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance, CapabilityPlugin,
    ExecutionError, ExecutionResult, Permission, Plugin, PluginMetadata,
};
use serde_json::json;
use std::sync::Arc;
use crate::scheduler::connector_resolver::{Connector, ConnectorDescriptor};

pub struct ShellPlugin;

impl Plugin for ShellPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "fusion-connector-shell".into(),
            version: semver::Version::parse("0.1.0").unwrap(),
            api_version: semver::Version::parse("0.1.0").unwrap(),
            min_compiler_version: semver::Version::parse("0.9.0").unwrap(),
            capabilities: vec![CapabilityId::new("shell.exec")],
        }
    }
}

impl CapabilityPlugin for ShellPlugin {
    fn capabilities(&self) -> Vec<CapabilityContract> {
        vec![CapabilityContract {
            id: CapabilityId::new("shell.exec"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Executes shell commands (not implemented — fails closed until a sandboxed runtime exists)".into(),
            inputs_schema: json!({"type": "object", "properties": {"command": {"type": "string"}}}),
            outputs_schema: json!({"type": "object", "properties": {"stdout": {"type": "string"}}}),
            permissions: vec![Permission::Filesystem("**".into())],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 10,
            reliability_score: 0.99,
            supports_streaming: false,
            traits: vec![],
        }]
    }
}

#[async_trait]
impl CapabilityExecutor for ShellPlugin {
    async fn execute(
        &self,
        instance: &CapabilityInstance,
        input: serde_json::Value,
    ) -> Result<ExecutionResult, ExecutionError> {
        let cmd = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutionError {
                connector: "shell".into(),
                capability: instance.contract.id.clone(),
                reason: "Missing 'command' field".into(),
                retryable: false,
            })?;

        let _ = cmd;
        Err(ExecutionError {
            connector: "shell".into(),
            capability: instance.contract.id.clone(),
            reason: "shell.exec is not implemented: no sandboxed runtime exists yet; refusing to fabricate command output".into(),
            retryable: false,
        })
    }
}

pub struct ShellConnector {
    plugin: Arc<ShellPlugin>,
}

impl ShellConnector {
    pub fn new() -> Self {
        Self {
            plugin: Arc::new(ShellPlugin),
        }
    }
}

impl Default for ShellConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl Connector for ShellConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            name: "shell".into(),
            version: semver::Version::new(0, 10, 0),
            supported_capabilities: vec![CapabilityId::new("shell.exec")],
        }
    }

    fn executor(&self) -> Arc<dyn CapabilityExecutor> {
        self.plugin.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_plugin_api::CapabilityContract;

    fn make_instance() -> CapabilityInstance {
        CapabilityInstance {
            contract: CapabilityContract {
                id: CapabilityId::new("shell.exec"),
                version: semver::Version::parse("0.1.0").unwrap(),
                description: "test".into(),
                inputs_schema: json!({}),
                outputs_schema: json!({}),
                permissions: vec![],
                dependencies: vec![],
                estimated_cost_usd: 0.0,
                estimated_latency_ms: 1,
                reliability_score: 1.0,
                supports_streaming: false,
                traits: vec![],
            },
            runtime_params: json!({}),
        }
    }

    #[test]
    fn test_shell_connector_descriptor() {
        let connector = ShellConnector::new();
        let desc = connector.descriptor();
        assert_eq!(desc.name, "shell");
        assert_eq!(desc.supported_capabilities.len(), 1);
    }

    #[tokio::test]
    async fn test_execution_fails_closed_instead_of_fabricating() {
        let plugin = ShellPlugin;
        let err = plugin
            .execute(&make_instance(), json!({ "command": "rm -rf /" }))
            .await
            .unwrap_err();
        assert!(
            err.reason.contains("not implemented"),
            "unexpected reason: {}",
            err.reason
        );
        assert!(!err.retryable);
    }
}
