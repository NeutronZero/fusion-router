use crate::release::certification::{CertificationArtifact, CertificationContext};
use crate::release::fixture::FixtureKind;
use crate::release::fixture_loader::{discover_fixtures, load_fixture_manifest, FixtureLoader};
use crate::release::gate::*;
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Instant;

pub struct PluginGateConfig {
    pub fixture_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct PluginArtifact {
    pub name: String,
    pub version: semver::Version,
    pub sdk_version: semver::Version,
    pub capabilities: Vec<String>,
    pub exported_symbols: Vec<String>,
    pub valid_manifest: bool,
}

impl PluginArtifact {
    pub fn new(
        name: impl Into<String>,
        version: semver::Version,
        sdk_version: semver::Version,
        capabilities: Vec<String>,
        exported_symbols: Vec<String>,
        valid_manifest: bool,
    ) -> Self {
        Self {
            name: name.into(),
            version,
            sdk_version,
            capabilities,
            exported_symbols,
            valid_manifest,
        }
    }
}

impl CertificationArtifact for PluginArtifact {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &semver::Version {
        &self.version
    }

    fn schema_checks(&self, _ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError> {
        Ok(vec![GateCheck {
            name: "plugin-manifest-schema".into(),
            passed: self.valid_manifest,
            message: if self.valid_manifest {
                format!("plugin {} manifest schema valid", self.name)
            } else {
                format!("plugin {} manifest schema invalid", self.name)
            },
        }])
    }

    fn contract_checks(&self, ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError> {
        let sdk_compat = self.sdk_version.major == ctx.sdk_version.major
            && self.sdk_version.minor <= ctx.sdk_version.minor;
        let symbol_compat = self.exported_symbols.contains(&"create_plugin".to_string())
            || self
                .exported_symbols
                .contains(&"plugin_api_version".to_string());
        let caps_compat = !self.capabilities.is_empty();

        Ok(vec![
            GateCheck {
                name: "sdk-version-compatibility".into(),
                passed: sdk_compat,
                message: format!(
                    "plugin SDK v{} compatible with host SDK v{}",
                    self.sdk_version, ctx.sdk_version
                ),
            },
            GateCheck {
                name: "exported-symbols".into(),
                passed: symbol_compat,
                message: format!("exported symbols: {:?}", self.exported_symbols),
            },
            GateCheck {
                name: "capability-declarations".into(),
                passed: caps_compat,
                message: format!("declared capabilities: {:?}", self.capabilities),
            },
        ])
    }
}

pub trait PluginBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn discover(&self, ctx: &CertificationContext) -> Result<Vec<PluginArtifact>, GateError>;
    fn load(&self, path: &std::path::Path) -> Result<PluginArtifact, GateError>;
}

#[allow(dead_code)]
pub struct FilesystemPluginBackend {
    loader: FixtureLoader,
}

impl FilesystemPluginBackend {
    pub fn new(fixture_root: PathBuf) -> Self {
        Self {
            loader: FixtureLoader::new(fixture_root),
        }
    }
}

/// On-disk plugin manifest. Unknown or absent fields fail closed instead of
/// fabricating a healthy artifact (gate integrity).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginManifestFile {
    name: String,
    version: semver::Version,
    sdk_version: semver::Version,
    capabilities: Vec<String>,
    exported_symbols: Vec<String>,
    valid_manifest: bool,
}

impl PluginBackend for FilesystemPluginBackend {
    fn name(&self) -> &'static str {
        "filesystem"
    }

    fn discover(&self, _ctx: &CertificationContext) -> Result<Vec<PluginArtifact>, GateError> {
        let manifest = load_fixture_manifest(&self.loader)?;
        let entries = discover_fixtures(&manifest, FixtureKind::Plugins)?;
        let mut results = Vec::new();
        for entry in &entries {
            let full_path = self
                .loader
                .resolve(&PathBuf::from("tests/fixtures").join(&entry.path));
            results.push(self.load(&full_path)?);
        }
        Ok(results)
    }

    fn load(&self, path: &std::path::Path) -> Result<PluginArtifact, GateError> {
        let file = self.loader.resolve_manifest_file(path, "json", "plugin")?;
        let content = self.loader.read_to_string(&file)?;
        let manifest: PluginManifestFile = serde_json::from_str(&content).map_err(|e| {
            GateError::ExecutionFailed(format!("invalid plugin manifest {}: {e}", file.display()))
        })?;
        Ok(PluginArtifact::new(
            manifest.name,
            manifest.version,
            manifest.sdk_version,
            manifest.capabilities,
            manifest.exported_symbols,
            manifest.valid_manifest,
        ))
    }
}

pub struct PluginGate {
    backend: Box<dyn PluginBackend>,
    _config: PluginGateConfig,
    metadata: GateMetadata,
}

impl PluginGate {
    pub fn new(backend: Box<dyn PluginBackend>, config: PluginGateConfig) -> Self {
        Self {
            backend,
            _config: config,
            metadata: GateMetadata {
                id: GateId::Plugin1,
                category: GateCategory::Certification,
                required: true,
                introduced: semver::Version::new(0, 11, 0),
            },
        }
    }
}

#[async_trait]
impl ReleaseGate for PluginGate {
    fn id(&self) -> GateId {
        GateId::Plugin1
    }
    fn name(&self) -> &'static str {
        "Plugin Conformance"
    }
    fn description(&self) -> &'static str {
        "Verify plugin manifest, capability contracts, symbol exports, and initialization compatibility"
    }
    fn metadata(&self) -> &GateMetadata {
        &self.metadata
    }
    async fn run(&self, ctx: &GateContext) -> GateExecution {
        let start = Instant::now();
        let cert_ctx = CertificationContext::new(ctx.workspace_root.clone());

        let artifacts = match self.backend.discover(&cert_ctx) {
            Ok(arts) => arts,
            Err(e) => return GateExecution::ExecutionError(e),
        };

        if artifacts.is_empty() {
            return GateExecution::Success(GateResult {
                gate_id: GateId::Plugin1,
                passed: true,
                summary: "No plugins to certify".into(),
                details: vec![],
                duration: start.elapsed(),
            });
        }

        let mut all_checks = Vec::new();
        for artifact in &artifacts {
            match artifact.schema_checks(&cert_ctx) {
                Ok(mut checks) => all_checks.append(&mut checks),
                Err(e) => return GateExecution::ExecutionError(e),
            }
            match artifact.contract_checks(&cert_ctx) {
                Ok(mut checks) => all_checks.append(&mut checks),
                Err(e) => return GateExecution::ExecutionError(e),
            }
        }

        let passed = all_checks.iter().all(|c| c.passed);
        let summary = if passed {
            format!("{} plugins certified", artifacts.len())
        } else {
            let failed = all_checks.iter().filter(|c| !c.passed).count();
            format!("{failed} checks failed across {} plugins", artifacts.len())
        };

        GateExecution::Success(GateResult {
            gate_id: GateId::Plugin1,
            passed,
            summary,
            details: all_checks,
            duration: start.elapsed(),
        })
    }
}

// Mock backend for testing
#[cfg(test)]
pub struct MockPluginBackend {
    pub artifacts: Vec<PluginArtifact>,
    pub should_error: bool,
}

#[cfg(test)]
impl PluginBackend for MockPluginBackend {
    fn name(&self) -> &'static str {
        "mock"
    }
    fn discover(&self, _ctx: &CertificationContext) -> Result<Vec<PluginArtifact>, GateError> {
        if self.should_error {
            return Err(GateError::ExecutionFailed(
                "mock plugin backend error".into(),
            ));
        }
        Ok(self.artifacts.clone())
    }
    fn load(&self, _path: &std::path::Path) -> Result<PluginArtifact, GateError> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_gate_metadata() {
        let gate = PluginGate::new(
            Box::new(MockPluginBackend {
                artifacts: vec![],
                should_error: false,
            }),
            PluginGateConfig {
                fixture_root: PathBuf::from("."),
            },
        );
        let meta = gate.metadata();
        assert_eq!(meta.id, GateId::Plugin1);
        assert_eq!(meta.category, GateCategory::Certification);
        assert!(meta.required);
    }

    #[tokio::test]
    async fn test_plugin_gate_passing() {
        let artifact = PluginArtifact::new(
            "echo",
            semver::Version::new(0, 10, 0),
            semver::Version::new(0, 10, 0),
            vec!["echo".into()],
            vec!["create_plugin".into()],
            true,
        );
        let gate = PluginGate::new(
            Box::new(MockPluginBackend {
                artifacts: vec![artifact],
                should_error: false,
            }),
            PluginGateConfig {
                fixture_root: PathBuf::from("."),
            },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(result.passed());
    }

    #[tokio::test]
    async fn test_plugin_gate_invalid_sdk_ver() {
        let artifact = PluginArtifact::new(
            "echo",
            semver::Version::new(0, 10, 0),
            semver::Version::new(9, 0, 0), // Incompatible SDK major version
            vec!["echo".into()],
            vec!["create_plugin".into()],
            true,
        );
        let gate = PluginGate::new(
            Box::new(MockPluginBackend {
                artifacts: vec![artifact],
                should_error: false,
            }),
            PluginGateConfig {
                fixture_root: PathBuf::from("."),
            },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(!result.passed());
    }

    #[tokio::test]
    async fn test_plugin_gate_execution_error() {
        let gate = PluginGate::new(
            Box::new(MockPluginBackend {
                artifacts: vec![],
                should_error: true,
            }),
            PluginGateConfig {
                fixture_root: PathBuf::from("."),
            },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(result.is_error());
    }

    fn temp_plugin_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("fusion_plugin_gate_{tag}_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("plugins/echo")).unwrap();
        dir
    }

    #[test]
    fn test_filesystem_backend_loads_real_fixture_content() {
        let temp = temp_plugin_dir("real");
        std::fs::write(
            temp.join("plugins/echo/plugin.json"),
            r#"{
                "name": "echo",
                "version": "0.12.0",
                "sdk_version": "0.10.0",
                "capabilities": ["echo"],
                "exported_symbols": ["create_plugin"],
                "valid_manifest": true
            }"#,
        )
        .unwrap();

        let backend = FilesystemPluginBackend::new(temp.clone());
        let artifact = backend.load(&temp.join("plugins/echo")).unwrap();
        assert_eq!(artifact.name, "echo");
        assert_eq!(artifact.version, semver::Version::new(0, 12, 0));
        assert_eq!(artifact.sdk_version, semver::Version::new(0, 10, 0));
        assert_eq!(artifact.capabilities, vec!["echo".to_string()]);
        assert_eq!(artifact.exported_symbols, vec!["create_plugin".to_string()]);
        assert!(artifact.valid_manifest);

        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn test_filesystem_backend_rejects_corrupt_manifest() {
        let temp = temp_plugin_dir("corrupt");
        std::fs::write(temp.join("plugins/echo/plugin.json"), "{ not json ]").unwrap();

        let backend = FilesystemPluginBackend::new(temp.clone());
        let result = backend.load(&temp.join("plugins/echo"));
        assert!(
            result.is_err(),
            "unparseable manifest must fail the gate, not fabricate a healthy artifact"
        );

        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn test_filesystem_backend_rejects_missing_required_fields() {
        let temp = temp_plugin_dir("missing");
        // Missing sdk_version / capabilities / exported_symbols / valid_manifest.
        std::fs::write(
            temp.join("plugins/echo/plugin.json"),
            r#"{ "name": "echo", "version": "0.10.0" }"#,
        )
        .unwrap();

        let backend = FilesystemPluginBackend::new(temp.clone());
        let result = backend.load(&temp.join("plugins/echo"));
        assert!(
            result.is_err(),
            "missing required fields must be a hard error"
        );

        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn test_filesystem_backend_rejects_unknown_fields() {
        let temp = temp_plugin_dir("unknown");
        std::fs::write(
            temp.join("plugins/echo/plugin.json"),
            r#"{
                "name": "echo",
                "version": "0.10.0",
                "sdk_version": "0.10.0",
                "capabilities": ["echo"],
                "exported_symbols": ["create_plugin"],
                "valid_manifest": true,
                "healthy_because_i_said_so": true
            }"#,
        )
        .unwrap();

        let backend = FilesystemPluginBackend::new(temp.clone());
        let result = backend.load(&temp.join("plugins/echo"));
        assert!(
            result.is_err(),
            "unknown self-declared fields must not be silently accepted"
        );

        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn test_filesystem_backend_missing_path_is_gate_error() {
        let temp = temp_plugin_dir("absent");
        let backend = FilesystemPluginBackend::new(temp.clone());
        let result = backend.load(&temp.join("plugins/ghost"));
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(temp);
    }
}
