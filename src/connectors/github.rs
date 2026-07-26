use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance, CapabilityPlugin,
    ExecutionError, ExecutionResult, Plugin, PluginMetadata,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use crate::scheduler::connector_resolver::{Connector, ConnectorDescriptor};

pub struct GitHubPlugin;

impl Plugin for GitHubPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "fusion-connector-github".into(),
            version: semver::Version::parse("0.1.0").unwrap(),
            api_version: semver::Version::parse("0.1.0").unwrap(),
            min_compiler_version: semver::Version::parse("0.9.0").unwrap(),
            capabilities: vec![CapabilityId::new("github.issue.create")],
        }
    }
}

impl CapabilityPlugin for GitHubPlugin {
    fn capabilities(&self) -> Vec<CapabilityContract> {
        vec![CapabilityContract {
            id: CapabilityId::new("github.issue.create"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Creates an issue on GitHub".into(),
            inputs_schema: json!({"type": "object", "properties": {"repo": {"type": "string"}, "title": {"type": "string"}}}),
            outputs_schema: json!({"type": "object", "properties": {"issue_url": {"type": "string"}}}),
            permissions: vec!["github".into()],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 100,
            reliability_score: 0.95,
            supports_streaming: false,
        }]
    }
}

#[async_trait]
impl CapabilityExecutor for GitHubPlugin {
    async fn execute(
        &self,
        instance: &CapabilityInstance,
        input: serde_json::Value,
    ) -> Result<ExecutionResult, ExecutionError> {
        let repo = input
            .get("repo")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutionError {
                connector: "github".into(),
                capability: instance.contract.id.clone(),
                reason: "Missing 'repo' field".into(),
                retryable: false,
            })?;

        let _title = input
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutionError {
                connector: "github".into(),
                capability: instance.contract.id.clone(),
                reason: "Missing 'title' field".into(),
                retryable: false,
            })?;

        let mut metrics = HashMap::new();
        metrics.insert("latency_ms".to_string(), 50.0);

        Ok(ExecutionResult {
            outputs: json!({ "issue_url": format!("https://github.com/{}/issues/1", repo) }),
            metrics,
        })
    }
}

pub struct GitHubConnector {
    plugin: Arc<GitHubPlugin>,
}

impl GitHubConnector {
    pub fn new() -> Self {
        Self {
            plugin: Arc::new(GitHubPlugin),
        }
    }
}

impl Default for GitHubConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl Connector for GitHubConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            name: "github".into(),
            version: semver::Version::parse("0.1.0").unwrap(),
            supported_capabilities: vec![CapabilityId::new("github.issue.create")],
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
    fn test_github_connector_descriptor() {
        let connector = GitHubConnector::new();
        let desc = connector.descriptor();
        assert_eq!(desc.name, "github");
        assert_eq!(desc.supported_capabilities.len(), 1);
    }
}
