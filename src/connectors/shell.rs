//! Phase 7B — `ShellConnector` (`src/connectors/shell.rs`)

use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance, CapabilityPlugin,
    ExecutionError, ExecutionResult, Plugin, PluginMetadata,
};
use serde_json::json;
use std::collections::HashMap;
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
            description: "Executes shell commands in a sandboxed runtime".into(),
            inputs_schema: json!({"type": "object", "properties": {"command": {"type": "string"}}}),
            outputs_schema: json!({"type": "object", "properties": {"stdout": {"type": "string"}}}),
            permissions: vec!["shell".into()],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 10,
            reliability_score: 0.99,
            supports_streaming: false,
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

        let mut metrics = HashMap::new();
        metrics.insert("latency_ms".to_string(), 5.0);

        Ok(ExecutionResult {
            outputs: json!({ "stdout": format!("Executing command: {}", cmd) }),
            metrics,
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
            version: semver::Version::parse("0.1.0").unwrap(),
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

    #[test]
    fn test_shell_connector_descriptor() {
        let connector = ShellConnector::new();
        let desc = connector.descriptor();
        assert_eq!(desc.name, "shell");
        assert_eq!(desc.supported_capabilities.len(), 1);
    }
}
