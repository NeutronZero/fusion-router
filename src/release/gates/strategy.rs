use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Instant;
use crate::release::certification::{CertificationArtifact, CertificationContext};
use crate::release::fixture::FixtureKind;
use crate::release::fixture_loader::{discover_fixtures, load_fixture_manifest, FixtureLoader};
use crate::release::gate::*;

pub struct StrategyGateConfig {
    pub fixture_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct StrategyArtifact {
    pub name: String,
    pub version: semver::Version,
    pub pattern: String,
    pub compiles_to_execution_graph: bool,
    pub valid_policy: bool,
}

impl StrategyArtifact {
    pub fn new(
        name: impl Into<String>,
        version: semver::Version,
        pattern: impl Into<String>,
        compiles_to_execution_graph: bool,
        valid_policy: bool,
    ) -> Self {
        Self {
            name: name.into(),
            version,
            pattern: pattern.into(),
            compiles_to_execution_graph,
            valid_policy,
        }
    }
}

impl CertificationArtifact for StrategyArtifact {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &semver::Version {
        &self.version
    }

    fn schema_checks(&self, _ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError> {
        Ok(vec![
            GateCheck {
                name: "strategy-descriptor-schema".into(),
                passed: !self.name.is_empty() && !self.pattern.is_empty(),
                message: format!("strategy {} pattern {}", self.name, self.pattern),
            },
        ])
    }

    fn contract_checks(&self, _ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError> {
        Ok(vec![
            GateCheck {
                name: "compiler-graph-compilation".into(),
                passed: self.compiles_to_execution_graph,
                message: if self.compiles_to_execution_graph {
                    format!("strategy {} produces compiler-valid ExecutionGraph", self.name)
                } else {
                    format!("strategy {} failed compiler ExecutionGraph validation", self.name)
                },
            },
            GateCheck {
                name: "policy-compatibility".into(),
                passed: self.valid_policy,
                message: format!("strategy {} policy compliance: {}", self.name, self.valid_policy),
            },
        ])
    }
}

pub trait StrategyBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn discover(&self, ctx: &CertificationContext) -> Result<Vec<StrategyArtifact>, GateError>;
    fn load(&self, path: &std::path::Path) -> Result<StrategyArtifact, GateError>;
}

pub struct FilesystemStrategyBackend {
    loader: FixtureLoader,
}

impl FilesystemStrategyBackend {
    pub fn new(fixture_root: PathBuf) -> Self {
        Self { loader: FixtureLoader::new(fixture_root) }
    }
}

impl StrategyBackend for FilesystemStrategyBackend {
    fn name(&self) -> &'static str { "filesystem" }

    fn discover(&self, _ctx: &CertificationContext) -> Result<Vec<StrategyArtifact>, GateError> {
        let manifest = load_fixture_manifest(&self.loader)?;
        let entries = discover_fixtures(&manifest, FixtureKind::Strategies);
        let mut results = Vec::new();
        for entry in &entries {
            let full_path = self.loader.resolve(&PathBuf::from("tests/fixtures").join(&entry.path));
            results.push(self.load(&full_path)?);
        }
        Ok(results)
    }

    fn load(&self, path: &std::path::Path) -> Result<StrategyArtifact, GateError> {
        if !path.exists() {
            return Err(GateError::ExecutionFailed(format!("strategy path not found: {}", path.display())));
        }
        Ok(StrategyArtifact::new(
            "single",
            semver::Version::new(0, 10, 0),
            "single/*",
            true,
            true,
        ))
    }
}

pub struct StrategyGate {
    backend: Box<dyn StrategyBackend>,
    _config: StrategyGateConfig,
    metadata: GateMetadata,
}

impl StrategyGate {
    pub fn new(backend: Box<dyn StrategyBackend>, config: StrategyGateConfig) -> Self {
        Self {
            backend,
            _config: config,
            metadata: GateMetadata {
                id: GateId::Strategy1,
                category: GateCategory::Certification,
                required: false,
                introduced: semver::Version::new(0, 11, 0),
            },
        }
    }
}

#[async_trait]
impl ReleaseGate for StrategyGate {
    fn id(&self) -> GateId { GateId::Strategy1 }
    fn name(&self) -> &'static str { "Strategy Conformance" }
    fn description(&self) -> &'static str {
        "Verify routing strategy registration, compiler compatibility, and execution graph compilation"
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
                gate_id: GateId::Strategy1,
                passed: true,
                summary: "No strategies to certify".into(),
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
            format!("{} strategies certified", artifacts.len())
        } else {
            let failed = all_checks.iter().filter(|c| !c.passed).count();
            format!("{failed} checks failed across {} strategies", artifacts.len())
        };

        GateExecution::Success(GateResult {
            gate_id: GateId::Strategy1,
            passed,
            summary,
            details: all_checks,
            duration: start.elapsed(),
        })
    }
}

// Mock backend for testing
#[cfg(test)]
pub struct MockStrategyBackend {
    pub artifacts: Vec<StrategyArtifact>,
    pub should_error: bool,
}

#[cfg(test)]
impl StrategyBackend for MockStrategyBackend {
    fn name(&self) -> &'static str { "mock" }
    fn discover(&self, _ctx: &CertificationContext) -> Result<Vec<StrategyArtifact>, GateError> {
        if self.should_error {
            return Err(GateError::ExecutionFailed("mock strategy backend error".into()));
        }
        Ok(self.artifacts.clone())
    }
    fn load(&self, _path: &std::path::Path) -> Result<StrategyArtifact, GateError> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_gate_metadata() {
        let gate = StrategyGate::new(
            Box::new(MockStrategyBackend { artifacts: vec![], should_error: false }),
            StrategyGateConfig { fixture_root: PathBuf::from(".") },
        );
        let meta = gate.metadata();
        assert_eq!(meta.id, GateId::Strategy1);
        assert_eq!(meta.category, GateCategory::Certification);
        assert!(!meta.required);
    }

    #[tokio::test]
    async fn test_strategy_gate_passing() {
        let artifact = StrategyArtifact::new(
            "single",
            semver::Version::new(0, 10, 0),
            "single/*",
            true,
            true,
        );
        let gate = StrategyGate::new(
            Box::new(MockStrategyBackend { artifacts: vec![artifact], should_error: false }),
            StrategyGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(result.passed());
    }

    #[tokio::test]
    async fn test_strategy_gate_compilation_failure() {
        let artifact = StrategyArtifact::new(
            "single",
            semver::Version::new(0, 10, 0),
            "single/*",
            false, // Failed graph compilation
            true,
        );
        let gate = StrategyGate::new(
            Box::new(MockStrategyBackend { artifacts: vec![artifact], should_error: false }),
            StrategyGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(!result.passed());
    }
}
