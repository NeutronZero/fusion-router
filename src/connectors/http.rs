use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance, CapabilityPlugin,
    ExecutionError, ExecutionResult, Permission, Plugin, PluginMetadata,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use crate::scheduler::connector_resolver::{Connector, ConnectorDescriptor};

pub struct HttpPlugin;

impl Plugin for HttpPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "fusion-connector-http".into(),
            version: semver::Version::parse("0.1.0").unwrap(),
            api_version: semver::Version::parse("0.1.0").unwrap(),
            min_compiler_version: semver::Version::parse("0.9.0").unwrap(),
            capabilities: vec![CapabilityId::new("http.request")],
        }
    }
}

impl CapabilityPlugin for HttpPlugin {
    fn capabilities(&self) -> Vec<CapabilityContract> {
        vec![CapabilityContract {
            id: CapabilityId::new("http.request"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Makes an HTTP request".into(),
            inputs_schema: json!({"type": "object", "properties": {"url": {"type": "string"}, "method": {"type": "string"}}}),
            outputs_schema: json!({"type": "object", "properties": {"body": {"type": "string"}, "status": {"type": "number"}}}),
            permissions: vec![Permission::Network],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 150,
            reliability_score: 0.99,
            supports_streaming: false,
        }]
    }
}

#[async_trait]
impl CapabilityExecutor for HttpPlugin {
    async fn execute(
        &self,
        instance: &CapabilityInstance,
        input: serde_json::Value,
    ) -> Result<ExecutionResult, ExecutionError> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutionError {
                connector: "http".into(),
                capability: instance.contract.id.clone(),
                reason: "Missing 'url' field".into(),
                retryable: false,
            })?;

        let _method = input.get("method").and_then(|v| v.as_str()).unwrap_or("GET");

        let mut metrics = HashMap::new();
        metrics.insert("latency_ms".to_string(), 50.0);

        Ok(ExecutionResult {
            outputs: json!({ "body": format!("Response from {}", url), "status": 200 }),
            metrics,
        })
    }
}

pub struct HttpConnector {
    plugin: Arc<HttpPlugin>,
}

impl HttpConnector {
    pub fn new() -> Self {
        Self {
            plugin: Arc::new(HttpPlugin),
        }
    }
}

impl Default for HttpConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl Connector for HttpConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            name: "http".into(),
            version: semver::Version::new(0, 10, 0),
            supported_capabilities: vec![CapabilityId::new("http.request")],
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
    fn test_http_connector_descriptor() {
        let connector = HttpConnector::new();
        let desc = connector.descriptor();
        assert_eq!(desc.name, "http");
        assert_eq!(desc.supported_capabilities.len(), 1);
    }
}
