use crate::scheduler::connector_resolver::{Connector, ConnectorDescriptor};
use crate::security::paths::canonicalize_within;
use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance, CapabilityPlugin,
    ExecutionError, ExecutionResult, Permission, Plugin, PluginMetadata,
};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Reads files from the filesystem (real I/O, Law 10 path containment).
///
/// The trust root defaults to the process working directory; any candidate
/// path must canonicalize inside it. Missing or escaping paths fail closed.
pub struct FilesystemPlugin {
    root: PathBuf,
}

impl Default for FilesystemPlugin {
    fn default() -> Self {
        Self {
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

impl Plugin for FilesystemPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "fusion-connector-filesystem".into(),
            version: semver::Version::new(0, 1, 0),
            api_version: semver::Version::new(0, 1, 0),
            min_compiler_version: semver::Version::new(0, 9, 0),
            capabilities: vec![CapabilityId::new("fs.read")],
        }
    }
}

impl CapabilityPlugin for FilesystemPlugin {
    fn capabilities(&self) -> Vec<CapabilityContract> {
        vec![CapabilityContract {
            id: CapabilityId::new("fs.read"),
            version: semver::Version::new(0, 1, 0),
            description: "Reads a file from the filesystem".into(),
            inputs_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            outputs_schema: json!({"type": "object", "properties": {"content": {"type": "string"}}}),
            permissions: vec![Permission::Filesystem("**".into())],
            dependencies: vec![],
            estimated_cost: fusion_core::NanoUSD::ZERO,
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

        let canonical =
            canonicalize_within(&self.root, std::path::Path::new(path)).map_err(|err| {
                ExecutionError {
                    connector: "filesystem".into(),
                    capability: instance.contract.id.clone(),
                    reason: format!("path rejected (Law 10): {err}"),
                    retryable: false,
                }
            })?;

        let started = std::time::Instant::now();
        let content =
            tokio::fs::read_to_string(&canonical)
                .await
                .map_err(|err| ExecutionError {
                    connector: "filesystem".into(),
                    capability: instance.contract.id.clone(),
                    reason: format!("failed to read {}: {err}", canonical.display()),
                    retryable: false,
                })?;

        let mut metrics = HashMap::new();
        metrics.insert(
            "latency_ms".to_string(),
            started.elapsed().as_secs_f64() * 1000.0,
        );

        Ok(ExecutionResult {
            outputs: json!({ "content": content }),
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
            plugin: Arc::new(FilesystemPlugin::default()),
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
    use fusion_plugin_api::CapabilityContract;

    fn make_instance() -> CapabilityInstance {
        CapabilityInstance {
            contract: CapabilityContract {
                id: CapabilityId::new("fs.read"),
                version: semver::Version::parse("0.1.0").unwrap(),
                description: "test".into(),
                inputs_schema: json!({}),
                outputs_schema: json!({}),
                permissions: vec![],
                dependencies: vec![],
                estimated_cost: fusion_core::NanoUSD::ZERO,
                estimated_latency_ms: 1,
                reliability_score: 1.0,
                supports_streaming: false,
                traits: vec![],
            },
            runtime_params: json!({}),
        }
    }

    fn temp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("_fusion_fs_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_filesystem_connector_descriptor() {
        let connector = FilesystemConnector::new();
        let desc = connector.descriptor();
        assert_eq!(desc.name, "filesystem");
        assert_eq!(desc.supported_capabilities.len(), 1);
    }

    #[tokio::test]
    async fn test_reads_real_file_content() {
        let root = temp_root();
        let file = root.join("a.txt");
        std::fs::write(&file, "hello from disk").unwrap();
        let plugin = FilesystemPlugin { root };
        let result = plugin
            .execute(&make_instance(), json!({ "path": file.to_str().unwrap() }))
            .await
            .unwrap();
        assert_eq!(result.outputs["content"], "hello from disk");
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[tokio::test]
    async fn test_rejects_path_escaping_trust_root() {
        let root = temp_root();
        std::fs::write(root.join("inside.txt"), "x").unwrap();
        let escape = format!("{}\\..\\..\\escape.txt", root.display());
        let plugin = FilesystemPlugin { root };
        let err = plugin
            .execute(&make_instance(), json!({ "path": escape }))
            .await
            .unwrap_err();
        assert!(
            err.reason.contains("Law 10"),
            "unexpected reason: {}",
            err.reason
        );
        std::fs::remove_dir_all(plugin.root).unwrap();
    }

    #[tokio::test]
    async fn test_missing_file_returns_error() {
        let root = temp_root();
        let plugin = FilesystemPlugin { root };
        let missing = format!("{}\\nope.txt", plugin.root.display());
        let err = plugin
            .execute(&make_instance(), json!({ "path": missing }))
            .await
            .unwrap_err();
        assert!(!err.reason.is_empty());
        let _ = std::fs::remove_dir_all(&plugin.root);
    }
}
