use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Instant;
use crate::release::fixture::{ExpectedOutcome, FixtureKind, FixtureManifest};
use crate::release::fixture_loader::{discover_fixtures, load_fixture_manifest, FixtureLoader};
use crate::release::gate::*;
use crate::config::AppConfig;

pub struct UpgradeGateConfig {
    pub fixture_root: PathBuf,
}

#[derive(Clone)]
pub struct ConfigFixture {
    pub version: semver::Version,
    pub path: PathBuf,
    pub expected: ExpectedOutcome,
    #[cfg(test)]
    pub content: Option<String>,
}

pub struct UpgradeContext {
    pub root: PathBuf,
    pub manifest: Option<FixtureManifest>,
}

pub trait UpgradeBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn discover_configs(&self, ctx: &UpgradeContext) -> Result<Vec<ConfigFixture>, GateError>;
    fn load_config(&self, fixture: &ConfigFixture) -> Result<String, GateError>;
}

pub struct FilesystemUpgradeBackend {
    loader: FixtureLoader,
}

impl FilesystemUpgradeBackend {
    pub fn new(fixture_root: PathBuf) -> Self {
        Self { loader: FixtureLoader::new(fixture_root) }
    }
}

impl UpgradeBackend for FilesystemUpgradeBackend {
    fn name(&self) -> &'static str { "filesystem" }

    fn discover_configs(&self, _ctx: &UpgradeContext) -> Result<Vec<ConfigFixture>, GateError> {
        let manifest = load_fixture_manifest(&self.loader)?;
        let entries = discover_fixtures(&manifest, FixtureKind::Configs);
        let mut results = Vec::new();
        for entry in &entries {
            results.push(ConfigFixture {
                version: entry.version.clone(),
                path: entry.path.clone(),
                expected: entry.expected.clone(),
                #[cfg(test)]
                content: None,
            });
        }
        Ok(results)
    }

    fn load_config(&self, fixture: &ConfigFixture) -> Result<String, GateError> {
        let full_path = self.loader.resolve(&PathBuf::from("tests/fixtures").join(&fixture.path));
        let files = self.loader.find_files(&full_path, "yaml")?;
        files.first()
            .map(|p| self.loader.read_to_string(p))
            .unwrap_or_else(|| Err(GateError::ExecutionFailed(format!("no yaml config found in {:?}", fixture.path))))
    }
}

pub struct UpgradeGate {
    backend: Box<dyn UpgradeBackend>,
    config: UpgradeGateConfig,
    metadata: GateMetadata,
}

impl UpgradeGate {
    pub fn new(backend: Box<dyn UpgradeBackend>, config: UpgradeGateConfig) -> Self {
        Self {
            backend,
            config,
            metadata: GateMetadata {
                id: GateId::Upgrade1,
                category: GateCategory::Upgrade,
                required: false,
                introduced: semver::Version::new(0, 11, 0),
            },
        }
    }
}

#[async_trait]
impl ReleaseGate for UpgradeGate {
    fn id(&self) -> GateId { GateId::Upgrade1 }
    fn name(&self) -> &'static str { "Configuration Upgrade" }
    fn description(&self) -> &'static str {
        "Verify historical configs load correctly through the current parser"
    }
    fn metadata(&self) -> &GateMetadata {
        &self.metadata
    }
    async fn run(&self, _ctx: &GateContext) -> GateExecution {
        let start = Instant::now();
        let upgrade_ctx = UpgradeContext {
            root: self.config.fixture_root.clone(),
            manifest: None,
        };
        let fixtures = match self.backend.discover_configs(&upgrade_ctx) {
            Ok(f) => f,
            Err(e) => return GateExecution::ExecutionError(e),
        };
        if fixtures.is_empty() {
            return GateExecution::Success(GateResult {
                gate_id: GateId::Upgrade1,
                passed: true,
                summary: "No configs to check".into(),
                details: vec![],
                duration: start.elapsed(),
            });
        }
        let mut all_checks = Vec::new();
        let mut all_passed = true;
        for fixture in &fixtures {
            let content = match self.backend.load_config(fixture) {
                Ok(c) => c,
                Err(e) => return GateExecution::ExecutionError(e),
            };
            let parse_result: Result<AppConfig, _> = serde_yaml::from_str(&content);
            let mut validation_errors: Vec<String> = Vec::new();
            if let Ok(config) = &parse_result {
                if let Err(errors) = config.validate() {
                    for e in &errors {
                        validation_errors.push(format!("{}: {}", e.field, e.message));
                    }
                }
            }
            let has_errors = parse_result.is_err() || !validation_errors.is_empty();
            let check_passed = match fixture.expected {
                ExpectedOutcome::Pass => !has_errors,
                ExpectedOutcome::Warning => true,
                ExpectedOutcome::Fail => has_errors,
            };
            if !check_passed { all_passed = false; }
            let status = if check_passed { "PASS" } else { "FAIL" };
            let detail = match fixture.expected {
                ExpectedOutcome::Pass => {
                    if has_errors {
                        format!("expected pass but got errors: {}", validation_errors.join("; "))
                    } else { "ok".into() }
                }
                ExpectedOutcome::Warning => {
                    if has_errors {
                        format!("warnings (expected): {}", validation_errors.join("; "))
                    } else { "no warnings (expected some)".into() }
                }
                ExpectedOutcome::Fail => {
                    if has_errors {
                        format!("expected failure: {}", validation_errors.join("; "))
                    } else { "expected fail but passed (regression)".into() }
                }
            };
            all_checks.push(GateCheck {
                name: format!("config-v{}", fixture.version),
                passed: check_passed,
                message: format!("{status} | {detail}"),
            });
        }
        let summary = if all_passed {
            format!("{} configs compatible", fixtures.len())
        } else {
            let failed = all_checks.iter().filter(|c| !c.passed).count();
            format!("{failed} configs failed compatibility check")
        };
        GateExecution::Success(GateResult {
            gate_id: GateId::Upgrade1,
            passed: all_passed,
            summary,
            details: all_checks,
            duration: start.elapsed(),
        })
    }
}

// Mock backend for testing
#[cfg(test)]
pub struct MockUpgradeBackend {
    pub configs: Vec<ConfigFixture>,
}

#[cfg(test)]
impl UpgradeBackend for MockUpgradeBackend {
    fn name(&self) -> &'static str { "mock" }
    fn discover_configs(&self, _ctx: &UpgradeContext) -> Result<Vec<ConfigFixture>, GateError> {
        Ok(self.configs.clone())
    }
    fn load_config(&self, fixture: &ConfigFixture) -> Result<String, GateError> {
        fixture.content.clone().ok_or_else(|| GateError::ExecutionFailed("no content".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::fixture::*;

    #[test]
    fn test_upgrade_gate_metadata() {
        let gate = UpgradeGate::new(
            Box::new(MockUpgradeBackend { configs: vec![] }),
            UpgradeGateConfig { fixture_root: PathBuf::from(".") },
        );
        let meta = gate.metadata();
        assert_eq!(meta.id, GateId::Upgrade1);
        assert_eq!(meta.category, GateCategory::Upgrade);
        assert!(!meta.required);
    }

    #[tokio::test]
    async fn test_upgrade_gate_passing_config() {
        let backend = MockUpgradeBackend { configs: vec![
            ConfigFixture {
                version: semver::Version::new(0, 10, 0),
                path: PathBuf::from("configs/v0.10"),
                expected: ExpectedOutcome::Pass,
                content: Some(r#"
server:
  host: "0.0.0.0"
  port: 8080
  shutdown_timeout_secs: 30
resources:
  max_daily_cost: 100.0
  max_daily_tokens: 1000000
"#.into()),
            },
        ]};
        let gate = UpgradeGate::new(
            Box::new(backend),
            UpgradeGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: Some(semver::Version::new(0, 11, 0)),
        };
        let result = gate.run(&ctx).await;
        assert!(result.passed());
    }

    #[tokio::test]
    async fn test_upgrade_gate_expected_fail_but_passes() {
        let backend = MockUpgradeBackend { configs: vec![
            ConfigFixture {
                version: semver::Version::new(0, 10, 0),
                path: PathBuf::from("configs/v0.10"),
                expected: ExpectedOutcome::Fail,
                content: Some(r#"
server:
  host: "0.0.0.0"
  port: 8080
  shutdown_timeout_secs: 30
resources:
  max_daily_cost: 100.0
  max_daily_tokens: 1000000
"#.into()),
            },
        ]};
        let gate = UpgradeGate::new(
            Box::new(backend),
            UpgradeGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: Some(semver::Version::new(0, 11, 0)),
        };
        let result = gate.run(&ctx).await;
        assert!(!result.passed());
    }

    #[tokio::test]
    async fn test_upgrade_gate_expected_warning() {
        let backend = MockUpgradeBackend { configs: vec![
            ConfigFixture {
                version: semver::Version::new(0, 9, 0),
                path: PathBuf::from("configs/v0.9"),
                expected: ExpectedOutcome::Warning,
                content: Some("server:\n  port: 0\nresources:\n  max_daily_cost: 100.0\n  max_daily_tokens: 1000000\n".into()),
            },
        ]};
        let gate = UpgradeGate::new(
            Box::new(backend),
            UpgradeGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: Some(semver::Version::new(0, 11, 0)),
        };
        let result = gate.run(&ctx).await;
        assert!(result.passed());
    }
}
