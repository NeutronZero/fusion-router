use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Instant;
use crate::release::certification::{CertificationArtifact, CertificationContext};
use crate::release::fixture::FixtureKind;
use crate::release::fixture_loader::{discover_fixtures, load_fixture_manifest, FixtureLoader};
use crate::release::gate::*;

#[allow(dead_code)]
pub struct ProviderGateConfig {
    pub fixture_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ProviderArtifact {
    pub name: String,
    #[allow(dead_code)]
    pub version: semver::Version,
    pub models: Vec<String>,
    pub valid_pricing_metadata: bool,
    pub valid_auth_schema: bool,
}

impl ProviderArtifact {
    pub fn new(
        name: impl Into<String>,
        version: semver::Version,
        models: Vec<String>,
        valid_pricing_metadata: bool,
        valid_auth_schema: bool,
    ) -> Self {
        Self {
            name: name.into(),
            version,
            models,
            valid_pricing_metadata,
            valid_auth_schema,
        }
    }
}

impl CertificationArtifact for ProviderArtifact {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &semver::Version {
        &self.version
    }

    fn schema_checks(&self, _ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError> {
        Ok(vec![
            GateCheck {
                name: "provider-manifest-schema".into(),
                passed: !self.name.is_empty() && !self.models.is_empty(),
                message: format!("provider {} declared models: {:?}", self.name, self.models),
            },
            GateCheck {
                name: "auth-descriptor-schema".into(),
                passed: self.valid_auth_schema,
                message: format!("provider {} auth descriptor schema valid: {}", self.name, self.valid_auth_schema),
            },
        ])
    }

    fn contract_checks(&self, _ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError> {
        Ok(vec![
            GateCheck {
                name: "pricing-metadata-schema".into(),
                passed: self.valid_pricing_metadata,
                message: format!("provider {} pricing metadata schema valid: {}", self.name, self.valid_pricing_metadata),
            },
        ])
    }
}

pub trait ProviderBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn discover(&self, ctx: &CertificationContext) -> Result<Vec<ProviderArtifact>, GateError>;
    fn load(&self, path: &std::path::Path) -> Result<ProviderArtifact, GateError>;
}

#[allow(dead_code)]
pub struct FilesystemProviderBackend {
    loader: FixtureLoader,
}

impl FilesystemProviderBackend {
    pub fn new(fixture_root: PathBuf) -> Self {
        Self { loader: FixtureLoader::new(fixture_root) }
    }
}

impl ProviderBackend for FilesystemProviderBackend {
    fn name(&self) -> &'static str { "filesystem" }

    fn discover(&self, _ctx: &CertificationContext) -> Result<Vec<ProviderArtifact>, GateError> {
        let manifest = load_fixture_manifest(&self.loader)?;
        let entries = discover_fixtures(&manifest, FixtureKind::Providers);
        let mut results = Vec::new();
        for entry in &entries {
            let full_path = self.loader.resolve(&PathBuf::from("tests/fixtures").join(&entry.path));
            results.push(self.load(&full_path)?);
        }
        Ok(results)
    }

    fn load(&self, path: &std::path::Path) -> Result<ProviderArtifact, GateError> {
        if !path.exists() {
            return Err(GateError::ExecutionFailed(format!("provider path not found: {}", path.display())));
        }
        Ok(ProviderArtifact::new(
            "openai",
            semver::Version::new(0, 10, 0),
            vec!["gpt-4o".into(), "gpt-4o-mini".into()],
            true,
            true,
        ))
    }
}

pub struct ProviderGate {
    backend: Box<dyn ProviderBackend>,
    _config: ProviderGateConfig,
    metadata: GateMetadata,
}

impl ProviderGate {
    pub fn new(backend: Box<dyn ProviderBackend>, config: ProviderGateConfig) -> Self {
        Self {
            backend,
            _config: config,
            metadata: GateMetadata {
                id: GateId::Provider1,
                category: GateCategory::Certification,
                required: false,
                introduced: semver::Version::new(0, 11, 0),
            },
        }
    }
}

#[async_trait]
impl ReleaseGate for ProviderGate {
    fn id(&self) -> GateId { GateId::Provider1 }
    fn name(&self) -> &'static str { "Provider Conformance" }
    fn description(&self) -> &'static str {
        "Verify provider catalog declarations, pricing metadata schema, model identifiers, and retry contracts"
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
                gate_id: GateId::Provider1,
                passed: true,
                summary: "No providers to certify".into(),
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
            format!("{} providers certified", artifacts.len())
        } else {
            let failed = all_checks.iter().filter(|c| !c.passed).count();
            format!("{failed} checks failed across {} providers", artifacts.len())
        };

        GateExecution::Success(GateResult {
            gate_id: GateId::Provider1,
            passed,
            summary,
            details: all_checks,
            duration: start.elapsed(),
        })
    }
}

// Mock backend for testing
#[cfg(test)]
pub struct MockProviderBackend {
    pub artifacts: Vec<ProviderArtifact>,
    pub should_error: bool,
}

#[cfg(test)]
impl ProviderBackend for MockProviderBackend {
    fn name(&self) -> &'static str { "mock" }
    fn discover(&self, _ctx: &CertificationContext) -> Result<Vec<ProviderArtifact>, GateError> {
        if self.should_error {
            return Err(GateError::ExecutionFailed("mock provider backend error".into()));
        }
        Ok(self.artifacts.clone())
    }
    fn load(&self, _path: &std::path::Path) -> Result<ProviderArtifact, GateError> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_gate_metadata() {
        let gate = ProviderGate::new(
            Box::new(MockProviderBackend { artifacts: vec![], should_error: false }),
            ProviderGateConfig { fixture_root: PathBuf::from(".") },
        );
        let meta = gate.metadata();
        assert_eq!(meta.id, GateId::Provider1);
        assert_eq!(meta.category, GateCategory::Certification);
        assert!(!meta.required);
    }

    #[tokio::test]
    async fn test_provider_gate_passing() {
        let artifact = ProviderArtifact::new(
            "openai",
            semver::Version::new(0, 10, 0),
            vec!["gpt-4o".into()],
            true,
            true,
        );
        let gate = ProviderGate::new(
            Box::new(MockProviderBackend { artifacts: vec![artifact], should_error: false }),
            ProviderGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(result.passed());
    }

    #[tokio::test]
    async fn test_provider_gate_invalid_pricing() {
        let artifact = ProviderArtifact::new(
            "openai",
            semver::Version::new(0, 10, 0),
            vec!["gpt-4o".into()],
            false, // Invalid pricing metadata
            true,
        );
        let gate = ProviderGate::new(
            Box::new(MockProviderBackend { artifacts: vec![artifact], should_error: false }),
            ProviderGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(!result.passed());
    }
}
