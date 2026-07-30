use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance, CapabilityPlugin,
    ExecutionError, ExecutionResult, Plugin, PluginMetadata,
};
use serde_json::json;
use std::collections::HashMap;

pub struct EchoPlugin;

impl EchoPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EchoPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for EchoPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "fusion-plugin-echo".into(),
            version: semver::Version::parse("0.1.0").unwrap(),
            api_version: semver::Version::parse("0.1.0").unwrap(),
            min_compiler_version: semver::Version::parse("0.9.0").unwrap(),
            capabilities: vec![
                CapabilityId::new("echo.text"),
                CapabilityId::new("echo.uppercase"),
            ],
        }
    }
}

impl CapabilityPlugin for EchoPlugin {
    fn capabilities(&self) -> Vec<CapabilityContract> {
        vec![
            CapabilityContract {
                id: CapabilityId::new("echo.text"),
                version: semver::Version::parse("0.1.0").unwrap(),
                description: "Echoes input text verbatim".into(),
                inputs_schema: json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" }
                    },
                    "required": ["text"]
                }),
                outputs_schema: json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" }
                    }
                }),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
                estimated_latency_ms: 1,
                reliability_score: 1.0,
                supports_streaming: false,
            },
            CapabilityContract {
                id: CapabilityId::new("echo.uppercase"),
                version: semver::Version::parse("0.1.0").unwrap(),
                description: "Echoes input text transformed to uppercase".into(),
                inputs_schema: json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" }
                    },
                    "required": ["text"]
                }),
                outputs_schema: json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" }
                    }
                }),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
                estimated_latency_ms: 1,
                reliability_score: 1.0,
                supports_streaming: false,
            },
        ]
    }
}

#[async_trait]
impl CapabilityExecutor for EchoPlugin {
    async fn execute(
        &self,
        instance: &CapabilityInstance,
        input: serde_json::Value,
    ) -> Result<ExecutionResult, ExecutionError> {
        let text = input
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutionError {
                connector: "echo".into(),
                capability: instance.contract.id.clone(),
                reason: "Missing required 'text' field".into(),
                retryable: false,
            })?;

        let outputs = match instance.contract.id.as_str() {
            "echo.text" => json!({ "text": text }),
            "echo.uppercase" => json!({ "text": text.to_uppercase() }),
            other => {
                return Err(ExecutionError {
                    connector: "echo".into(),
                    capability: instance.contract.id.clone(),
                    reason: format!("Unknown capability ID: {}", other),
                    retryable: false,
                })
            }
        };

        let mut metrics = HashMap::new();
        metrics.insert("latency_ms".to_string(), 1.0);

        Ok(ExecutionResult { outputs, metrics })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo_plugin_execution() {
        let plugin = EchoPlugin::new();
        let caps = plugin.capabilities();
        assert_eq!(caps.len(), 2);

        let instance = CapabilityInstance {
            contract: caps[0].clone(),
            runtime_params: json!({}),
        };

        let result = plugin
            .execute(&instance, json!({ "text": "hello fusion" }))
            .await
            .unwrap();

        assert_eq!(result.outputs["text"], "hello fusion");
    }
}
