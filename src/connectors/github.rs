use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance, CapabilityPlugin,
    ExecutionError, ExecutionResult, Permission, Plugin, PluginMetadata,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use crate::scheduler::connector_resolver::{Connector, ConnectorDescriptor};

/// Creates GitHub issues for real via the REST API (`POST /repos/{repo}/issues`).
///
/// Requires the `GITHUB_TOKEN` environment variable; without it the connector
/// fails closed rather than fabricating an issue URL.
pub struct GitHubPlugin {
    client: reqwest::Client,
}

impl Default for GitHubPlugin {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Plugin for GitHubPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "fusion-connector-github".into(),
            version: semver::Version::new(0, 1, 0),
            api_version: semver::Version::new(0, 1, 0),
            min_compiler_version: semver::Version::new(0, 9, 0),
            capabilities: vec![CapabilityId::new("github.issue.create")],
        }
    }
}

impl CapabilityPlugin for GitHubPlugin {
    fn capabilities(&self) -> Vec<CapabilityContract> {
        vec![CapabilityContract {
            id: CapabilityId::new("github.issue.create"),
            version: semver::Version::new(0, 1, 0),
            description: "Creates an issue on GitHub".into(),
            inputs_schema: json!({"type": "object", "properties": {"repo": {"type": "string"}, "title": {"type": "string"}}}),
            outputs_schema: json!({"type": "object", "properties": {"issue_url": {"type": "string"}}}),
            permissions: vec![Permission::Http("https://api.github.com".into())],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 100,
            reliability_score: 0.95,
            supports_streaming: false,
            traits: vec![],
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

        let title = input
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutionError {
                connector: "github".into(),
                capability: instance.contract.id.clone(),
                reason: "Missing 'title' field".into(),
                retryable: false,
            })?;

        let token = std::env::var("GITHUB_TOKEN").map_err(|_| ExecutionError {
            connector: "github".into(),
            capability: instance.contract.id.clone(),
            reason: "GITHUB_TOKEN environment variable not set; refusing to fabricate an issue URL".into(),
            retryable: false,
        })?;

        let started = std::time::Instant::now();
        let response = self
            .client
            .post(format!("https://api.github.com/repos/{repo}/issues"))
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", "fusion-router")
            .header("Accept", "application/vnd.github+json")
            .json(&json!({ "title": title }))
            .send()
            .await
            .map_err(|err| ExecutionError {
                connector: "github".into(),
                capability: instance.contract.id.clone(),
                reason: format!("GitHub API request failed: {err}"),
                retryable: true,
            })?;

        let status = response.status();
        let payload: Value = response.json().await.map_err(|err| ExecutionError {
            connector: "github".into(),
            capability: instance.contract.id.clone(),
            reason: format!("GitHub API returned an unparseable response: {err}"),
            retryable: false,
        })?;

        if !status.is_success() {
            return Err(ExecutionError {
                connector: "github".into(),
                capability: instance.contract.id.clone(),
                reason: format!(
                    "GitHub API rejected issue creation ({}): {}",
                    status.as_u16(),
                    payload
                ),
                retryable: status.is_server_error(),
            });
        }

        let issue_url = payload
            .get("html_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutionError {
                connector: "github".into(),
                capability: instance.contract.id.clone(),
                reason: "GitHub API response did not include html_url".into(),
                retryable: false,
            })?;

        let mut metrics = HashMap::new();
        metrics.insert("latency_ms".to_string(), started.elapsed().as_secs_f64() * 1000.0);

        Ok(ExecutionResult {
            outputs: json!({ "issue_url": issue_url }),
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
            plugin: Arc::new(GitHubPlugin::default()),
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
            version: semver::Version::new(0, 10, 0),
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
    use fusion_plugin_api::CapabilityContract;

    fn make_instance() -> CapabilityInstance {
        CapabilityInstance {
            contract: CapabilityContract {
                id: CapabilityId::new("github.issue.create"),
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
    fn test_github_connector_descriptor() {
        let connector = GitHubConnector::new();
        let desc = connector.descriptor();
        assert_eq!(desc.name, "github");
        assert_eq!(desc.supported_capabilities.len(), 1);
    }

    #[tokio::test]
    async fn test_missing_token_fails_closed() {
        std::env::remove_var("GITHUB_TOKEN");
        let plugin = GitHubPlugin::default();
        let err = plugin
            .execute(
                &make_instance(),
                json!({ "repo": "octocat/Hello-World", "title": "t" }),
            )
            .await
            .unwrap_err();
        assert!(
            err.reason.contains("GITHUB_TOKEN"),
            "unexpected reason: {}",
            err.reason
        );
        assert!(!err.retryable);
    }
}
