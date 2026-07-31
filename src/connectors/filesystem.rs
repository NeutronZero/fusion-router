use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance, CapabilityPlugin,
    ExecutionError, ExecutionResult, Permission, Plugin, PluginMetadata,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use crate::scheduler::connector_resolver::{Connector, ConnectorDescriptor};

pub struct FilesystemPlugin;

impl Plugin for FilesystemPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "fusion-connector-filesystem".into(),
            version: semver::Version::parse("0.1.0").unwrap(),
            api_version: semver::Version::parse("0.1.0").unwrap(),
            min_compiler_version: semver::Version::parse("0.9.0").unwrap(),
            capabilities: vec![CapabilityId::new("fs.read")],
        }
    }
}

impl CapabilityPlugin for FilesystemPlugin {
    fn capabilities(&self) -> Vec<CapabilityContract> {
        vec![CapabilityContract {
            id: CapabilityId::new("fs.read"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Reads a file from the filesystem".into(),
            inputs_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            outputs_schema: json!({"type": "object", "properties": {"content": {"type": "string"}}}),
            permissions: vec![Permission::Filesystem("**".into())],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 5,
            reliability_score: 0.999,
            supports_streaming: false,
            traits: vec![],
        }]
    }
}

#[async_trait]
impl CapabilityExecutor for FilesystemPlugin {
    async fn execute(
        &self,
        instance: &CapabilityInstance,
        input: serde_json::Value,
    ) -> Result<ExecutionResult, ExecutionError> {
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutionError {
                connector: "filesystem".into(),
                capability: instance.contract.id.clone(),
                reason: "Missing 'path' field".into(),
                retryable: false,
            })?;

        let mut metrics = HashMap::new();
        metrics.insert("latency_ms".to_string(), 1.0);

        Ok(ExecutionResult {
            outputs: json!({ "content": format!("Content of {}", path) }),
            metrics,
        })
    }
}

pub struct FilesystemConnector {
    plugin: Arc<FilesystemPlugin>,
}

impl FilesystemConnector {
    pub fn new() -> Self {
        Self {
            plugin: Arc::new(FilesystemPlugin),
        }
    }
}

impl Default for FilesystemConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl Connector for FilesystemConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            name: "filesystem".into(),
            version: semver::Version::new(0, 10, 0),
            supported_capabilities: vec![CapabilityId::new("fs.read")],
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
    fn test_filesystem_connector_descriptor() {
        let connector = FilesystemConnector::new();
        let desc = connector.descriptor();
        assert_eq!(desc.name, "filesystem");
        assert_eq!(desc.supported_capabilities.len(), 1);
    }
}
