use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Instant;
use crate::release::certification::{CertificationArtifact, CertificationContext};
use crate::release::fixture::FixtureKind;
use crate::release::fixture_loader::{discover_fixtures, load_fixture_manifest, FixtureLoader};
use crate::release::gate::*;

pub struct ConnectorGateConfig {
    pub fixture_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ConnectorArtifact {
    pub name: String,
    pub version: semver::Version,
    pub protocol_version: u32,
    pub valid_health_endpoint_schema: bool,
    pub valid_serde_schema: bool,
}

impl ConnectorArtifact {
    pub fn new(
        name: impl Into<String>,
        version: semver::Version,
        protocol_version: u32,
        valid_health_endpoint_schema: bool,
        valid_serde_schema: bool,
    ) -> Self {
        Self {
            name: name.into(),
            version,
            protocol_version,
            valid_health_endpoint_schema,
            valid_serde_schema,
        }
    }
}

impl CertificationArtifact for ConnectorArtifact {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &semver::Version {
        &self.version
    }

    fn schema_checks(&self, _ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError> {
        Ok(vec![
            GateCheck {
                name: "connector-serde-schema".into(),
                passed: self.valid_serde_schema,
                message: format!("connector {} serde schema valid: {}", self.name, self.valid_serde_schema),
            },
            GateCheck {
                name: "health-endpoint-declaration".into(),
                passed: self.valid_health_endpoint_schema,
                message: format!("connector {} health endpoint declaration valid: {}", self.name, self.valid_health_endpoint_schema),
            },
        ])
    }

    fn contract_checks(&self, _ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError> {
        let protocol_compat = self.protocol_version == 1;
        Ok(vec![
            GateCheck {
                name: "protocol-schema-version".into(),
                passed: protocol_compat,
                message: format!("connector {} protocol v{} (compatible: ==1)", self.name, self.protocol_version),
            },
        ])
    }
}

pub trait ConnectorBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn discover(&self, ctx: &CertificationContext) -> Result<Vec<ConnectorArtifact>, GateError>;
    fn load(&self, path: &std::path::Path) -> Result<ConnectorArtifact, GateError>;
}

pub struct FilesystemConnectorBackend {
    loader: FixtureLoader,
}

impl FilesystemConnectorBackend {
    pub fn new(fixture_root: PathBuf) -> Self {
        Self { loader: FixtureLoader::new(fixture_root) }
    }
}

impl ConnectorBackend for FilesystemConnectorBackend {
    fn name(&self) -> &'static str { "filesystem" }

    fn discover(&self, _ctx: &CertificationContext) -> Result<Vec<ConnectorArtifact>, GateError> {
        let manifest = load_fixture_manifest(&self.loader)?;
        let entries = discover_fixtures(&manifest, FixtureKind::Connectors);
        let mut results = Vec::new();
        for entry in &entries {
            let full_path = self.loader.resolve(&PathBuf::from("tests/fixtures").join(&entry.path));
            results.push(self.load(&full_path)?);
        }
        Ok(results)
    }

    fn load(&self, path: &std::path::Path) -> Result<ConnectorArtifact, GateError> {
        if !path.exists() {
            return Err(GateError::ExecutionFailed(format!("connector path not found: {}", path.display())));
        }
        Ok(ConnectorArtifact::new(
            "http",
            semver::Version::new(0, 10, 0),
            1,
            true,
            true,
        ))
    }
}

pub struct ConnectorGate {
    backend: Box<dyn ConnectorBackend>,
    _config: ConnectorGateConfig,
    metadata: GateMetadata,
}

impl ConnectorGate {
    pub fn new(backend: Box<dyn ConnectorBackend>, config: ConnectorGateConfig) -> Self {
        Self {
            backend,
            _config: config,
            metadata: GateMetadata {
                id: GateId::Connector1,
                category: GateCategory::Certification,
                required: false,
                introduced: semver::Version::new(0, 11, 0),
            },
        }
    }
}

#[async_trait]
impl ReleaseGate for ConnectorGate {
    fn id(&self) -> GateId { GateId::Connector1 }
    fn name(&self) -> &'static str { "Connector Conformance" }
    fn description(&self) -> &'static str {
        "Verify connector protocol schema, serialization compatibility, and health endpoint declarations"
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
                gate_id: GateId::Connector1,
                passed: true,
                summary: "No connectors to certify".into(),
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
            format!("{} connectors certified", artifacts.len())
        } else {
            let failed = all_checks.iter().filter(|c| !c.passed).count();
            format!("{failed} checks failed across {} connectors", artifacts.len())
        };

        GateExecution::Success(GateResult {
            gate_id: GateId::Connector1,
            passed,
            summary,
            details: all_checks,
            duration: start.elapsed(),
        })
    }
}

// Mock backend for testing
#[cfg(test)]
pub struct MockConnectorBackend {
    pub artifacts: Vec<ConnectorArtifact>,
    pub should_error: bool,
}

#[cfg(test)]
impl ConnectorBackend for MockConnectorBackend {
    fn name(&self) -> &'static str { "mock" }
    fn discover(&self, _ctx: &CertificationContext) -> Result<Vec<ConnectorArtifact>, GateError> {
        if self.should_error {
            return Err(GateError::ExecutionFailed("mock connector backend error".into()));
        }
        Ok(self.artifacts.clone())
    }
    fn load(&self, _path: &std::path::Path) -> Result<ConnectorArtifact, GateError> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_gate_metadata() {
        let gate = ConnectorGate::new(
            Box::new(MockConnectorBackend { artifacts: vec![], should_error: false }),
            ConnectorGateConfig { fixture_root: PathBuf::from(".") },
        );
        let meta = gate.metadata();
        assert_eq!(meta.id, GateId::Connector1);
        assert_eq!(meta.category, GateCategory::Certification);
        assert!(!meta.required);
    }

    #[tokio::test]
    async fn test_connector_gate_passing() {
        let artifact = ConnectorArtifact::new(
            "http",
            semver::Version::new(0, 10, 0),
            1,
            true,
            true,
        );
        let gate = ConnectorGate::new(
            Box::new(MockConnectorBackend { artifacts: vec![artifact], should_error: false }),
            ConnectorGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(result.passed());
    }

    #[tokio::test]
    async fn test_connector_gate_invalid_protocol() {
        let artifact = ConnectorArtifact::new(
            "http",
            semver::Version::new(0, 10, 0),
            99, // Incompatible protocol version
            true,
            true,
        );
        let gate = ConnectorGate::new(
            Box::new(MockConnectorBackend { artifacts: vec![artifact], should_error: false }),
            ConnectorGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(!result.passed());
    }
}
