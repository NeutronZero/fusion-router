use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Instant;
use crate::release::gate::*;

pub struct DeterminismGateConfig {
    pub fixture_root: PathBuf,
}

pub struct DeterminismContext {
    pub root: PathBuf,
    pub request_fixture: String,
}

pub trait DeterminismBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn compile_fixture(&self, ctx: &DeterminismContext) -> Result<u64, GateError>;
}

pub struct RealDeterminismBackend;

impl DeterminismBackend for RealDeterminismBackend {
    fn name(&self) -> &'static str { "real" }

    fn compile_fixture(&self, ctx: &DeterminismContext) -> Result<u64, GateError> {
        use std::hash::{Hash, Hasher};
        let planner = fusion_planner::IntentPlanner::new(fusion_core::ModelCatalog::default());
        let request = fusion_planner::PlanningRequest {
            intent: fusion_planner::ExecutionIntent::Balanced,
            user_prompt: ctx.request_fixture.clone(),
            requested_model: None,
            requested_strategy: None,
            strategy_config: None,
            requirements: Default::default(),
            policies: Default::default(),
            capability_catalog: Default::default(),
            model_catalog: Default::default(),
            telemetry: Default::default(),
        };
        let ir = planner.plan(&request).map_err(|e| GateError::ExecutionFailed(format!("planning failed: {e:?}")))?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        ir.to_canonical_json().map_err(|e| GateError::ExecutionFailed(e.to_string()))?.hash(&mut hasher);
        Ok(hasher.finish())
    }
}

pub struct DeterminismGate {
    backend: Box<dyn DeterminismBackend>,
    config: DeterminismGateConfig,
    metadata: GateMetadata,
}

impl DeterminismGate {
    pub fn new(backend: Box<dyn DeterminismBackend>, config: DeterminismGateConfig) -> Self {
        Self {
            backend,
            config,
            metadata: GateMetadata {
                id: GateId::Determinism1,
                category: GateCategory::Determinism,
                required: false,
                introduced: semver::Version::new(0, 11, 0),
            },
        }
    }
}

#[async_trait]
impl ReleaseGate for DeterminismGate {
    fn id(&self) -> GateId { GateId::Determinism1 }
    fn name(&self) -> &'static str { "Planner Determinism" }
    fn description(&self) -> &'static str {
        "Verify same planner input produces identical execution graphs"
    }
    fn metadata(&self) -> &GateMetadata {
        &self.metadata
    }
    async fn run(&self, _ctx: &GateContext) -> GateExecution {
        let start = Instant::now();
        let det_ctx = DeterminismContext {
            root: self.config.fixture_root.clone(),
            request_fixture: String::new(),
        };
        let hash1 = match self.backend.compile_fixture(&det_ctx) {
            Ok(h) => h,
            Err(e) => return GateExecution::ExecutionError(e),
        };
        let hash2 = match self.backend.compile_fixture(&det_ctx) {
            Ok(h) => h,
            Err(e) => return GateExecution::ExecutionError(e),
        };

        let passed = hash1 == hash2;
        let summary = if passed {
            format!("Deterministic: hash = {:016x}", hash1)
        } else {
            format!("Non-deterministic: hash1 = {:016x}, hash2 = {:016x}", hash1, hash2)
        };
        GateExecution::Success(GateResult {
            gate_id: GateId::Determinism1,
            passed,
            summary,
            details: vec![GateCheck {
                name: "compiler-determinism".into(),
                passed,
                message: if passed {
                    format!("Two compilations produced identical hash {:016x}", hash1)
                } else {
                    format!("Hash mismatch: {:016x} vs {:016x}", hash1, hash2)
                },
            }],
            duration: start.elapsed(),
        })
    }
}

// Mock backend for testing
#[cfg(test)]
pub struct MockDeterminismBackend {
    pub hash1: u64,
    pub hash2: u64,
    pub should_error: bool,
    call_count: std::sync::atomic::AtomicU32,
}

#[cfg(test)]
impl MockDeterminismBackend {
    pub fn new(hash1: u64, hash2: u64) -> Self {
        Self {
            hash1,
            hash2,
            should_error: false,
            call_count: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

#[cfg(test)]
impl DeterminismBackend for MockDeterminismBackend {
    fn name(&self) -> &'static str { "mock" }
    fn compile_fixture(&self, _ctx: &DeterminismContext) -> Result<u64, GateError> {
        if self.should_error {
            return Err(GateError::ExecutionFailed("mock error".into()));
        }
        let count = self.call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if count == 0 { Ok(self.hash1) } else { Ok(self.hash2) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determinism_gate_metadata() {
        let gate = DeterminismGate::new(
            Box::new(MockDeterminismBackend::new(1, 1)),
            DeterminismGateConfig { fixture_root: PathBuf::from(".") },
        );
        let meta = gate.metadata();
        assert_eq!(meta.id, GateId::Determinism1);
        assert_eq!(meta.category, GateCategory::Determinism);
        assert!(!meta.required);
    }

    #[tokio::test]
    async fn test_determinism_gate_identical_hashes() {
        let gate = DeterminismGate::new(
            Box::new(MockDeterminismBackend::new(42, 42)),
            DeterminismGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(result.passed());
        if let GateExecution::Success(res) = result {
            assert_eq!(res.details.len(), 1);
            assert!(res.details[0].passed);
        } else {
            panic!("expected GateExecution::Success");
        }
    }

    #[tokio::test]
    async fn test_determinism_gate_different_hashes() {
        let gate = DeterminismGate::new(
            Box::new(MockDeterminismBackend::new(42, 99)),
            DeterminismGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(!result.passed());
    }

    #[tokio::test]
    async fn test_determinism_gate_backend_error() {
        let gate = DeterminismGate::new(
            Box::new(MockDeterminismBackend {
                hash1: 0,
                hash2: 0,
                should_error: true,
                call_count: std::sync::atomic::AtomicU32::new(0),
            }),
            DeterminismGateConfig { fixture_root: PathBuf::from(".") },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(result.is_error());
    }
}
