use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance, CapabilityPlugin,
    ExecutionError, ExecutionResult, Permission, Plugin, PluginMetadata,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use crate::scheduler::connector_resolver::{Connector, ConnectorDescriptor};

pub struct McpPlugin;

impl Plugin for McpPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "fusion-connector-mcp".into(),
            version: semver::Version::parse("0.1.0").unwrap(),
            api_version: semver::Version::parse("0.1.0").unwrap(),
            min_compiler_version: semver::Version::parse("0.9.0").unwrap(),
            capabilities: vec![CapabilityId::new("mcp.tool.invoke")],
        }
    }
}

impl CapabilityPlugin for McpPlugin {
    fn capabilities(&self) -> Vec<CapabilityContract> {
        vec![CapabilityContract {
            id: CapabilityId::new("mcp.tool.invoke"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Invokes an MCP tool".into(),
            inputs_schema: json!({"type": "object", "properties": {"tool": {"type": "string"}}}),
            outputs_schema: json!({"type": "object", "properties": {"result": {"type": "string"}}}),
            permissions: vec![Permission::Network],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 200,
            reliability_score: 0.99,
            supports_streaming: false,
        }]
    }
}

#[async_trait]
impl CapabilityExecutor for McpPlugin {
    async fn execute(
        &self,
        instance: &CapabilityInstance,
        input: serde_json::Value,
    ) -> Result<ExecutionResult, ExecutionError> {
        let tool = input
            .get("tool")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutionError {
                connector: "mcp".into(),
                capability: instance.contract.id.clone(),
                reason: "Missing 'tool' field".into(),
                retryable: false,
            })?;

        let mut metrics = HashMap::new();
        metrics.insert("latency_ms".to_string(), 100.0);

        Ok(ExecutionResult {
            outputs: json!({ "result": format!("Result of {}", tool) }),
            metrics,
        })
    }
}

pub struct McpConnector {
    plugin: Arc<McpPlugin>,
}

impl McpConnector {
    pub fn new() -> Self {
        Self {
            plugin: Arc::new(McpPlugin),
        }
    }
}

impl Default for McpConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl Connector for McpConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            name: "mcp".into(),
            version: semver::Version::new(0, 10, 0),
            supported_capabilities: vec![CapabilityId::new("mcp.tool.invoke")],
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
    fn test_mcp_connector_descriptor() {
        let connector = McpConnector::new();
        let desc = connector.descriptor();
        assert_eq!(desc.name, "mcp");
        assert_eq!(desc.supported_capabilities.len(), 1);
    }
}
