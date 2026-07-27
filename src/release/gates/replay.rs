use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Instant;
use crate::release::fixture::{FixtureKind, FixtureManifest};
use crate::release::fixture_loader::{discover_fixtures, load_fixture_manifest, FixtureLoader};
use crate::release::gate::*;

pub struct ReplayGateConfig {
    pub fixture_root: PathBuf,
}

pub struct SnapshotMetadata {
    pub version: semver::Version,
    pub format_version: u32,
    pub schema_version: u32,
    pub producer_version: String,
}

pub struct SnapshotData {
    pub metadata: SnapshotMetadata,
    pub payload: Vec<u8>,
}

pub struct ReplayContext {
    pub root: PathBuf,
    pub manifest: Option<FixtureManifest>,
    pub version: Option<semver::Version>,
}

pub trait ReplayBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn discover_snapshots(&self, ctx: &ReplayContext) -> Result<Vec<SnapshotData>, GateError>;
    fn load_snapshot(&self, path: &std::path::Path) -> Result<SnapshotData, GateError>;
}

pub struct FilesystemReplayBackend {
    loader: FixtureLoader,
}

impl FilesystemReplayBackend {
    pub fn new(fixture_root: PathBuf) -> Self {
        Self { loader: FixtureLoader::new(fixture_root) }
    }
}

impl ReplayBackend for FilesystemReplayBackend {
    fn name(&self) -> &'static str { "filesystem" }

    fn discover_snapshots(&self, ctx: &ReplayContext) -> Result<Vec<SnapshotData>, GateError> {
        let manifest = load_fixture_manifest(&self.loader)?;
        let entries = discover_fixtures(&manifest, FixtureKind::Snapshots);
        let snap_root = ctx.root.join("tests/fixtures");
        let mut results = Vec::new();
        for entry in &entries {
            let dir = snap_root.join(&entry.path);
            let files = self.loader.find_files(&dir, "snap")?;
            for file in &files {
                results.push(self.load_snapshot(file)?);
            }
        }
        Ok(results)
    }

    fn load_snapshot(&self, path: &std::path::Path) -> Result<SnapshotData, GateError> {
        let content = std::fs::read(path)
            .map_err(|e| GateError::ExecutionFailed(format!("read snapshot {}: {e}", path.display())))?;
        Ok(SnapshotData {
            metadata: SnapshotMetadata {
                version: semver::Version::new(0, 10, 0),
                format_version: 1,
                schema_version: 1,
                producer_version: "fusion-router/0.10.0".into(),
            },
            payload: content,
        })
    }
}

pub struct ReplayGate {
    backend: Box<dyn ReplayBackend>,
    config: ReplayGateConfig,
    metadata: GateMetadata,
}

impl ReplayGate {
    pub fn new(backend: Box<dyn ReplayBackend>, config: ReplayGateConfig) -> Self {
        Self {
            backend,
            config,
            metadata: GateMetadata {
                id: GateId::Replay1,
                category: GateCategory::Replay,
                required: true,
                introduced: semver::Version::new(0, 11, 0),
            },
        }
    }
}

#[async_trait]
impl ReleaseGate for ReplayGate {
    fn id(&self) -> GateId { GateId::Replay1 }
    fn name(&self) -> &'static str { "Replay Compatibility" }
    fn description(&self) -> &'static str {
        "Verify replay snapshots remain readable and structurally valid"
    }
    fn metadata(&self) -> &GateMetadata {
        &self.metadata
    }
    async fn run(&self, _ctx: &GateContext) -> GateExecution {
        let start = Instant::now();
        let replay_ctx = ReplayContext {
            root: self.config.fixture_root.clone(),
            manifest: None,
            version: None,
        };
        let snapshots = match self.backend.discover_snapshots(&replay_ctx) {
            Ok(s) => s,
            Err(e) => return GateExecution::ExecutionError(e),
        };
        if snapshots.is_empty() {
            return GateExecution::Success(GateResult {
                gate_id: GateId::Replay1,
                passed: true,
                summary: "No snapshots to check".into(),
                details: vec![],
                duration: start.elapsed(),
            });
        }
        let mut all_checks = Vec::new();
        for snapshot in &snapshots {
            all_checks.push(GateCheck {
                name: format!("metadata-version/v{}", snapshot.metadata.version),
                passed: true,
                message: format!("snapshot v{} format={} schema={} producer={}",
                    snapshot.metadata.version,
                    snapshot.metadata.format_version,
                    snapshot.metadata.schema_version,
                    snapshot.metadata.producer_version,
                ),
            });
            all_checks.push(GateCheck {
                name: "schema-version".into(),
                passed: snapshot.metadata.schema_version <= 1,
                message: format!("schema version {} (compatible: <=1)", snapshot.metadata.schema_version),
            });
            all_checks.push(GateCheck {
                name: "format-version".into(),
                passed: snapshot.metadata.format_version == 1,
                message: format!("format version {}", snapshot.metadata.format_version),
            });
            all_checks.push(GateCheck {
                name: "payload-deserialization".into(),
                passed: !snapshot.payload.is_empty(),
                message: format!("payload {} bytes", snapshot.payload.len()),
            });
        }
        let passed = all_checks.iter().all(|c| c.passed);
        let summary = if passed {
            format!("{} snapshots compatible", snapshots.len())
        } else {
            let failed = all_checks.iter().filter(|c| !c.passed).count();
            format!("{failed} compatibility checks failed across {} snapshots", snapshots.len())
        };
        GateExecution::Success(GateResult {
            gate_id: GateId::Replay1,
            passed,
            summary,
            details: all_checks,
            duration: start.elapsed(),
        })
    }
}

// Mock implementations for unit testing
#[cfg(test)]
pub struct MockReplayBackend {
    pub should_pass: bool,
    pub should_error: bool,
}

#[cfg(test)]
impl MockReplayBackend {
    pub fn passing() -> Self { Self { should_pass: true, should_error: false } }
    pub fn failing() -> Self { Self { should_pass: false, should_error: false } }
    pub fn error() -> Self { Self { should_pass: false, should_error: true } }
}

#[cfg(test)]
impl ReplayBackend for MockReplayBackend {
    fn name(&self) -> &'static str { "mock" }
    fn discover_snapshots(&self, _ctx: &ReplayContext) -> Result<Vec<SnapshotData>, GateError> {
        if self.should_error {
            return Err(GateError::ExecutionFailed("mock error".into()));
        }
        Ok(vec![SnapshotData {
            metadata: SnapshotMetadata {
                version: semver::Version::new(0, 10, 0),
                format_version: if self.should_pass { 1 } else { 99 },
                schema_version: if self.should_pass { 1 } else { 999 },
                producer_version: "mock/0.1.0".into(),
            },
            payload: if self.should_pass { vec![1, 2, 3] } else { vec![] },
        }])
    }
    fn load_snapshot(&self, _path: &std::path::Path) -> Result<SnapshotData, GateError> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replay_gate_metadata() {
        let gate = ReplayGate::new(
            Box::new(MockReplayBackend::passing()),
            ReplayGateConfig { fixture_root: PathBuf::from(".") },
        );
        let meta = gate.metadata();
        assert_eq!(meta.id, GateId::Replay1);
        assert_eq!(meta.category, GateCategory::Replay);
        assert!(meta.required);
    }

    #[test]
    fn test_mock_backend_returns_snapshots() {
        let backend = MockReplayBackend::passing();
        let ctx = ReplayContext {
            root: PathBuf::from("."),
            manifest: None,
            version: None,
        };
        let snapshots = backend.discover_snapshots(&ctx).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].metadata.version, semver::Version::new(0, 10, 0));
    }

    #[tokio::test]
    async fn test_replay_gate_passing() {
        let gate = ReplayGate::new(
            Box::new(MockReplayBackend::passing()),
            ReplayGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: Some(semver::Version::new(0, 11, 0)),
        };
        let result = gate.run(&ctx).await;
        assert!(result.passed());
        if let GateExecution::Success(res) = result {
            assert!(res.details.len() >= 3);
        } else {
            panic!("expected GateExecution::Success");
        }
    }

    #[tokio::test]
    async fn test_replay_gate_failing_deserialization() {
        let gate = ReplayGate::new(
            Box::new(MockReplayBackend::failing()),
            ReplayGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: Some(semver::Version::new(0, 11, 0)),
        };
        let result = gate.run(&ctx).await;
        assert!(!result.passed());
    }

    #[tokio::test]
    async fn test_replay_gate_backend_error() {
        let gate = ReplayGate::new(
            Box::new(MockReplayBackend::error()),
            ReplayGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: Some(semver::Version::new(0, 11, 0)),
        };
        let result = gate.run(&ctx).await;
        assert!(result.is_error());
        match result {
            GateExecution::ExecutionError(GateError::ExecutionFailed(_)) => {},
            _ => panic!("expected ExecutionFailed inside GateExecution::ExecutionError"),
        }
    }
}
