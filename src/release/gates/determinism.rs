use crate::release::gate::*;
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Instant;

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
    fn name(&self) -> &'static str {
        "real"
    }

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
        let ir = planner
            .plan(&request)
            .map_err(|e| GateError::ExecutionFailed(format!("planning failed: {e:?}")))?;
        let types_ir =
            crate::ir::adapter::workflow_to_types(&ir).map_err(GateError::ExecutionFailed)?;
        let graph = fusion_compiler::lower_to_graph(types_ir)
            .map_err(|e| GateError::ExecutionFailed(format!("compilation failed: {e:?}")))?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        ir.to_canonical_json()
            .map_err(|e| GateError::ExecutionFailed(e.to_string()))?
            .hash(&mut hasher);
        serde_json::to_string(&graph)
            .map_err(|e| GateError::ExecutionFailed(e.to_string()))?
            .hash(&mut hasher);
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
                required: true,
                introduced: semver::Version::new(0, 11, 0),
            },
        }
    }

    /// Loads determinism fixtures from `<fixture_root>/tests/fixtures/determinism/`.
    /// Every `*.txt`, `*.json`, and `*.yaml` file becomes one prompt fixture.
    /// Missing or empty fixture sets are an error — determinism is never
    /// certified against an empty prompt (AD-006).
    fn load_fixtures(&self) -> Result<Vec<(String, String)>, GateError> {
        let dir = self
            .config
            .fixture_root
            .join("tests")
            .join("fixtures")
            .join("determinism");
        let mut fixtures: Vec<(String, String)> = Vec::new();
        if dir.is_dir() {
            let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(&dir)
                .map_err(|e| GateError::ExecutionFailed(format!("read {}: {e}", dir.display())))?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .filter(|e| {
                    matches!(
                        e.path().extension().and_then(|x| x.to_str()),
                        Some("txt") | Some("json") | Some("yaml") | Some("yml")
                    )
                })
                .collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let content = std::fs::read_to_string(entry.path()).map_err(|e| {
                    GateError::ExecutionFailed(format!("read {}: {e}", entry.path().display()))
                })?;
                if content.trim().is_empty() {
                    return Err(GateError::ExecutionFailed(format!(
                        "determinism fixture {} is empty",
                        entry.path().display()
                    )));
                }
                fixtures.push((entry.file_name().to_string_lossy().into_owned(), content));
            }
        }
        if fixtures.is_empty() {
            return Err(GateError::ExecutionFailed(format!(
                "no determinism fixtures found under {} — create tests/fixtures/determinism/*.txt so the gate certifies real planner input",
                dir.display()
            )));
        }
        Ok(fixtures)
    }
}

#[async_trait]
impl ReleaseGate for DeterminismGate {
    fn id(&self) -> GateId {
        GateId::Determinism1
    }
    fn name(&self) -> &'static str {
        "Planner Determinism"
    }
    fn description(&self) -> &'static str {
        "Verify same planner input produces identical execution graphs"
    }
    fn metadata(&self) -> &GateMetadata {
        &self.metadata
    }
    async fn run(&self, _ctx: &GateContext) -> GateExecution {
        let start = Instant::now();
        let fixtures = match self.load_fixtures() {
            Ok(f) => f,
            Err(e) => return GateExecution::ExecutionError(e),
        };

        let mut all_checks = Vec::new();
        let mut all_passed = true;
        for (name, content) in &fixtures {
            let det_ctx = DeterminismContext {
                root: self.config.fixture_root.clone(),
                request_fixture: content.clone(),
            };
            let hash1 = match self.backend.compile_fixture(&det_ctx) {
                Ok(h) => h,
                Err(e) => {
                    return GateExecution::ExecutionError(GateError::ExecutionFailed(format!(
                        "fixture {name}: {e}"
                    )))
                }
            };
            let hash2 = match self.backend.compile_fixture(&det_ctx) {
                Ok(h) => h,
                Err(e) => {
                    return GateExecution::ExecutionError(GateError::ExecutionFailed(format!(
                        "fixture {name}: {e}"
                    )))
                }
            };

            let passed = hash1 == hash2;
            all_passed &= passed;
            all_checks.push(GateCheck {
                name: format!("compiler-determinism/{name}"),
                passed,
                message: if passed {
                    format!("Two compilations produced identical hash {:016x}", hash1)
                } else {
                    format!("Hash mismatch: {:016x} vs {:016x}", hash1, hash2)
                },
            });
        }

        let summary = if all_passed {
            format!("Deterministic across {} fixture(s)", fixtures.len())
        } else {
            let failed = all_checks.iter().filter(|c| !c.passed).count();
            format!(
                "{failed} determinism check(s) failed across {} fixture(s)",
                fixtures.len()
            )
        };
        GateExecution::Success(GateResult {
            gate_id: GateId::Determinism1,
            passed: all_passed,
            summary,
            details: all_checks,
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
    fn name(&self) -> &'static str {
        "mock"
    }
    fn compile_fixture(&self, _ctx: &DeterminismContext) -> Result<u64, GateError> {
        if self.should_error {
            return Err(GateError::ExecutionFailed("mock error".into()));
        }
        let count = self
            .call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if count == 0 {
            Ok(self.hash1)
        } else {
            Ok(self.hash2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determinism_gate_metadata() {
        let gate = DeterminismGate::new(
            Box::new(MockDeterminismBackend::new(1, 1)),
            DeterminismGateConfig {
                fixture_root: PathBuf::from("."),
            },
        );
        let meta = gate.metadata();
        assert_eq!(meta.id, GateId::Determinism1);
        assert_eq!(meta.category, GateCategory::Determinism);
        assert!(meta.required);
    }

    #[tokio::test]
    async fn test_determinism_gate_missing_fixtures_fails_closed() {
        let temp = std::env::temp_dir().join(format!("_fusion_det_empty_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        let gate = DeterminismGate::new(
            Box::new(MockDeterminismBackend::new(42, 42)),
            DeterminismGateConfig {
                fixture_root: temp.clone(),
            },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(result.is_error(), "empty fixture set must be an error");
        let _ = std::fs::remove_dir_all(temp);
    }

    #[tokio::test]
    async fn test_determinism_gate_uses_fixture_content() {
        let temp = std::env::temp_dir().join(format!("_fusion_det_fx_{}", uuid::Uuid::new_v4()));
        let dir = temp.join("tests").join("fixtures").join("determinism");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("prompt.txt"), "Summarize the retry policy.").unwrap();

        struct CapturingBackend {
            fixtures_seen: std::sync::Mutex<Vec<String>>,
            hash: u64,
        }
        impl DeterminismBackend for CapturingBackend {
            fn name(&self) -> &'static str {
                "capturing"
            }
            fn compile_fixture(&self, ctx: &DeterminismContext) -> Result<u64, GateError> {
                self.fixtures_seen
                    .lock()
                    .unwrap()
                    .push(ctx.request_fixture.clone());
                Ok(self.hash)
            }
        }
        let backend = std::sync::Arc::new(CapturingBackend {
            fixtures_seen: std::sync::Mutex::new(Vec::new()),
            hash: 42,
        });
        // The gate owns its backend; share via Arc clone for inspection.
        struct ArcBackend(std::sync::Arc<CapturingBackend>);
        impl DeterminismBackend for ArcBackend {
            fn name(&self) -> &'static str {
                "capturing-arc"
            }
            fn compile_fixture(&self, ctx: &DeterminismContext) -> Result<u64, GateError> {
                self.0.compile_fixture(ctx)
            }
        }
        let gate = DeterminismGate::new(
            Box::new(ArcBackend(backend.clone())),
            DeterminismGateConfig {
                fixture_root: temp.clone(),
            },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(result.passed());
        let seen = backend.fixtures_seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "fixture compiled twice");
        assert_eq!(seen[0], "Summarize the retry policy.");
        let _ = std::fs::remove_dir_all(temp);
    }

    #[tokio::test]
    async fn test_determinism_gate_identical_hashes() {
        let gate = DeterminismGate::new(
            Box::new(MockDeterminismBackend::new(42, 42)),
            DeterminismGateConfig {
                fixture_root: PathBuf::from("."),
            },
        );
        let ctx = GateContext {
            workspace_root: PathBuf::from("."),
            baseline_version: None,
        };
        let result = gate.run(&ctx).await;
        assert!(result.passed());
        if let GateExecution::Success(res) = result {
            // One check per fixture under tests/fixtures/determinism.
            assert!(res.details.len() >= 1);
            assert!(res.details.iter().all(|d| d.passed));
        } else {
            panic!("expected GateExecution::Success");
        }
    }

    #[tokio::test]
    async fn test_determinism_gate_different_hashes() {
        let gate = DeterminismGate::new(
            Box::new(MockDeterminismBackend::new(42, 99)),
            DeterminismGateConfig {
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
    async fn test_determinism_gate_backend_error() {
        let gate = DeterminismGate::new(
            Box::new(MockDeterminismBackend {
                hash1: 0,
                hash2: 0,
                should_error: true,
                call_count: std::sync::atomic::AtomicU32::new(0),
            }),
            DeterminismGateConfig {
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
}
