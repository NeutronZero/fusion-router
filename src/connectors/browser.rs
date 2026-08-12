use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance, CapabilityPlugin,
    ExecutionError, ExecutionResult, Permission, Plugin, PluginMetadata,
};
use serde_json::json;
use std::sync::Arc;
use crate::scheduler::connector_resolver::{Connector, ConnectorDescriptor};

pub struct BrowserPlugin;

impl Plugin for BrowserPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "fusion-connector-browser".into(),
            version: semver::Version::new(0, 1, 0),
            api_version: semver::Version::new(0, 1, 0),
            min_compiler_version: semver::Version::new(0, 9, 0),
            capabilities: vec![CapabilityId::new("browser.navigate")],
        }
    }
}

impl CapabilityPlugin for BrowserPlugin {
    fn capabilities(&self) -> Vec<CapabilityContract> {
        vec![CapabilityContract {
            id: CapabilityId::new("browser.navigate"),
            version: semver::Version::new(0, 1, 0),
            description: "Navigates a browser to a URL (not implemented — fails closed until a browser driver exists)".into(),
            inputs_schema: json!({"type": "object", "properties": {"url": {"type": "string"}}}),
            outputs_schema: json!({"type": "object", "properties": {"content": {"type": "string"}}}),
            permissions: vec![Permission::Network],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 1000,
            reliability_score: 0.90,
            supports_streaming: false,
            traits: vec![],
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

        let _ = url;
        Err(ExecutionError {
            connector: "browser".into(),
            capability: instance.contract.id.clone(),
            reason: "browser.navigate is not implemented: no browser driver exists; refusing to fabricate page content".into(),
            retryable: false,
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
    use fusion_plugin_api::CapabilityContract;

    fn make_instance() -> CapabilityInstance {
        CapabilityInstance {
            contract: CapabilityContract {
                id: CapabilityId::new("browser.navigate"),
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
    fn test_browser_connector_descriptor() {
        let connector = BrowserConnector::new();
        let desc = connector.descriptor();
        assert_eq!(desc.name, "browser");
        assert_eq!(desc.supported_capabilities.len(), 1);
    }

    #[tokio::test]
    async fn test_execution_fails_closed_instead_of_fabricating() {
        let plugin = BrowserPlugin;
        let err = plugin
            .execute(&make_instance(), json!({ "url": "https://example.com" }))
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
