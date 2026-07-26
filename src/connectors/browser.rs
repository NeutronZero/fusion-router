use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance, CapabilityPlugin,
    ExecutionError, ExecutionResult, Plugin, PluginMetadata,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use crate::scheduler::connector_resolver::{Connector, ConnectorDescriptor};

pub struct BrowserPlugin;

impl Plugin for BrowserPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "fusion-connector-browser".into(),
            version: semver::Version::parse("0.1.0").unwrap(),
            api_version: semver::Version::parse("0.1.0").unwrap(),
            min_compiler_version: semver::Version::parse("0.9.0").unwrap(),
            capabilities: vec![CapabilityId::new("browser.navigate")],
        }
    }
}

impl CapabilityPlugin for BrowserPlugin {
    fn capabilities(&self) -> Vec<CapabilityContract> {
        vec![CapabilityContract {
            id: CapabilityId::new("browser.navigate"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Navigates a browser to a URL".into(),
            inputs_schema: json!({"type": "object", "properties": {"url": {"type": "string"}}}),
            outputs_schema: json!({"type": "object", "properties": {"content": {"type": "string"}}}),
            permissions: vec!["browser".into()],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 1000,
            reliability_score: 0.90,
            supports_streaming: false,
        }]
    }
}

#[async_trait]
impl CapabilityExecutor for BrowserPlugin {
    async fn execute(
        &self,
        instance: &CapabilityInstance,
        input: serde_json::Value,
    ) -> Result<ExecutionResult, ExecutionError> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutionError {
                connector: "browser".into(),
                capability: instance.contract.id.clone(),
                reason: "Missing 'url' field".into(),
                retryable: false,
            })?;

        let mut metrics = HashMap::new();
        metrics.insert("latency_ms".to_string(), 500.0);

        Ok(ExecutionResult {
            outputs: json!({ "content": format!("Content of {}", url) }),
            metrics,
        })
    }
}

pub struct BrowserConnector {
    plugin: Arc<BrowserPlugin>,
}

impl BrowserConnector {
    pub fn new() -> Self {
        Self {
            plugin: Arc::new(BrowserPlugin),
        }
    }
}

impl Default for BrowserConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl Connector for BrowserConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            name: "browser".into(),
            version: semver::Version::new(0, 10, 0),
            supported_capabilities: vec![CapabilityId::new("browser.navigate")],
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
    fn test_browser_connector_descriptor() {
        let connector = BrowserConnector::new();
        let desc = connector.descriptor();
        assert_eq!(desc.name, "browser");
        assert_eq!(desc.supported_capabilities.len(), 1);
    }
}
