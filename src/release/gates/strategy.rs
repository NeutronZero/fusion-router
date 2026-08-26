use crate::compiler::context::CompilationContext;
use crate::compiler::ir::{DebateRole, StrategyIR};
use crate::compiler::registry::StrategyRegistry;
use crate::release::certification::{CertificationArtifact, CertificationContext};
use crate::release::fixture::FixtureKind;
use crate::release::fixture_loader::{discover_fixtures, load_fixture_manifest, FixtureLoader};
use crate::release::gate::*;
use crate::strategies::{chain, consensus, debate, react, reflection, single};
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Instant;

#[allow(dead_code)]
pub struct StrategyGateConfig {
    pub fixture_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct StrategyArtifact {
    pub name: String,
    pub version: semver::Version,
    pub pattern: String,
    /// Computed by actually lowering the declared strategy through the host
    /// compiler conversion path — never trusted from fixture declarations.
    pub compiles_to_execution_graph: bool,
    /// Computed by parsing/normalizing/bridging the declared policy through
    /// `src/policy` types — never trusted from fixture declarations.
    pub valid_policy: bool,
    /// Diagnostic when compilation verification failed, if available.
    pub compile_error: Option<String>,
    /// Diagnostic when policy validation failed, if available.
    pub policy_error: Option<String>,
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
            compile_error: None,
            policy_error: None,
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
        Ok(vec![GateCheck {
            name: "strategy-descriptor-schema".into(),
            passed: !self.name.is_empty() && !self.pattern.is_empty(),
            message: format!("strategy {} pattern {}", self.name, self.pattern),
        }])
    }

    fn contract_checks(&self, _ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError> {
        let compile_msg = match (&self.compile_error, self.compiles_to_execution_graph) {
            (Some(err), false) => format!(
                "strategy {} failed compiler ExecutionGraph verification: {err}",
                self.name
            ),
            (None, true) => format!(
                "strategy {} lowered through the host compiler into a valid ExecutionGraph",
                self.name
            ),
            (None, false) => format!(
                "strategy {} failed compiler ExecutionGraph validation",
                self.name
            ),
            (Some(_), true) => unreachable!("error recorded only on failure"),
        };
        Ok(vec![
            GateCheck {
                name: "compiler-graph-compilation".into(),
                passed: self.compiles_to_execution_graph,
                message: compile_msg,
            },
            GateCheck {
                name: "policy-compatibility".into(),
                passed: self.valid_policy,
                message: match (&self.policy_error, self.valid_policy) {
                    (Some(err), false) => format!(
                        "strategy {} policy compliance check failed: {err}",
                        self.name
                    ),
                    (_, valid) => format!("strategy {} policy compliance: {}", self.name, valid),
                },
            },
        ])
    }
}

pub trait StrategyBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn discover(&self, ctx: &CertificationContext) -> Result<Vec<StrategyArtifact>, GateError>;
    fn load(&self, path: &std::path::Path) -> Result<StrategyArtifact, GateError>;
}

/// On-disk strategy manifest. Unknown or absent fields fail closed instead of
/// fabricating a pass. The historical self-declared booleans
/// (`compiles_to_execution_graph` / `valid_policy`) are no longer part of the
/// schema: the gate *computes* both from real compiler and policy evidence.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StrategyManifest {
    name: String,
    version: semver::Version,
    pattern: String,
    kind: crate::types::StrategyKind,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    config: std::collections::HashMap<String, serde_json::Value>,
    /// Optional policy declarations (PolicyAST JSON) the strategy ships with.
    /// Parsed, normalized, and bridged through `src/policy` to count as valid.
    #[serde(default)]
    policy: Option<serde_json::Value>,
}

pub struct FilesystemStrategyBackend {
    loader: FixtureLoader,
}

impl FilesystemStrategyBackend {
    pub fn new(fixture_root: PathBuf) -> Self {
        Self {
            loader: FixtureLoader::new(fixture_root),
        }
    }
}

/// Built-in host strategies available to the certification lowering path.
/// `Custom` kinds are deliberately absent — they fail closed exactly like the
/// crates strategy compiler (Gate 10).
fn builtin_strategy_registry() -> StrategyRegistry {
    let mut registry = StrategyRegistry::new();
    registry.register(std::sync::Arc::new(single::SingleStrategy));
    registry.register(std::sync::Arc::new(consensus::ConsensusStrategy::default()));
    registry.register(std::sync::Arc::new(
        reflection::ReflectionStrategy::default(),
    ));
    registry.register(std::sync::Arc::new(react::ReActStrategy::default()));
    registry.register(std::sync::Arc::new(chain::ChainStrategy {
        stages: vec![Box::new(single::SingleStrategy)],
    }));
    registry.register(std::sync::Arc::new(debate::DebateStrategy {
        debaters: vec![
            Box::new(single::SingleStrategy),
            Box::new(single::SingleStrategy),
        ],
        judge: Box::new(single::SingleStrategy),
    }));
    registry
}

/// Minimal representative `StrategyIR` for a declared strategy kind. Only used
/// as lowering input for built-in kinds; `Custom` never reaches here.
fn representative_ir(kind: &crate::types::StrategyKind) -> Option<StrategyIR> {
    use crate::types::StrategyKind as K;
    Some(match kind {
        K::Single => StrategyIR::Single,
        K::Consensus => StrategyIR::Consensus {
            count: 3,
            members: vec![],
        },
        K::Reflection => StrategyIR::Reflection { max_cycles: 3 },
        K::Debate => StrategyIR::Debate {
            roles: vec![
                DebateRole {
                    name: "defender".into(),
                    model: "default".into(),
                    stance: "pro".into(),
                },
                DebateRole {
                    name: "critic".into(),
                    model: "default".into(),
                    stance: "con".into(),
                },
            ],
        },
        K::ReAct => StrategyIR::ReAct { max_iterations: 5 },
        K::Chain => StrategyIR::Chain {
            stages: vec![StrategyIR::Single],
        },
        _ => return None,
    })
}

/// REAL compilation check (gate integrity): lower the declared strategy via the
/// registered host `Strategy` implementation into a `PrimitiveGraph`, then run
/// that graph carrying the fixture's kind + config through the host conversion
/// (`PrimitiveGraph::to_execution_graph`). Any failure is returned as an error
/// string so the gate check can fail with evidence instead of fabricating a
/// pass.
fn verify_strategy_compilation(manifest: &StrategyManifest) -> Result<usize, String> {
    let registry = builtin_strategy_registry();
    let label = manifest.kind.as_label().to_lowercase();
    let strategy = registry.get(&label).map_err(|d| {
        format!(
            "strategy kind '{}' is not registered for lowering: {}",
            label, d.message
        )
    })?;
    let ir = representative_ir(&manifest.kind).ok_or_else(|| {
        format!(
            "strategy kind '{}' has no registered delegate; refusing to certify",
            manifest.kind.as_label()
        )
    })?;
    let mut ctx = CompilationContext::new();
    if let Some(model) = &manifest.model {
        // Honor the declared model so lowering resolves against it.
        ctx.available_models.push(model.clone());
    }
    let primitive_graph = strategy.lower(&ir, &ctx).map_err(|d| d.message.clone())?;

    let retry_policy = crate::types::RetryPolicy {
        max_retries: 0,
        backoff_ms: 0,
    };
    let execution_graph = primitive_graph
        .to_execution_graph(
            manifest.kind.clone(),
            &retry_policy,
            &None,
            &manifest.config,
        )
        .map_err(|e| format!("PrimitiveGraph → ExecutionGraph conversion failed: {e}"))?;
    if execution_graph.nodes.is_empty() {
        return Err("lowering produced an empty ExecutionGraph".into());
    }
    Ok(execution_graph.nodes.len())
}

/// REAL policy check: parse the declared declarations JSON through the in-crate
/// policy parser, normalize to `PolicyIR`, and bridge it into the compiler's
/// policy IR type. No declaration present means nothing to violate.
fn verify_policy_declarations(manifest: &StrategyManifest) -> Result<(), String> {
    let declared = match &manifest.policy {
        None => return Ok(()),
        Some(value) => value,
    };
    let json = serde_json::to_string(declared)
        .map_err(|e| format!("policy declarations are not valid JSON: {e}"))?;
    let (ast, diagnostics) = crate::policy::ast::PolicyParser::parse_json(&json)
        .map_err(|e| format!("policy declarations failed to parse: {e}"))?;
    for diagnostic in &diagnostics {
        if diagnostic.severity == crate::policy::diagnostics::DiagnosticSeverity::Error {
            return Err(format!(
                "policy diagnostics reported an error: {}",
                diagnostic.message
            ));
        }
    }
    let policy_ir = crate::policy::ir::PolicyIR::from_ast(&ast)
        .map_err(|d| format!("policy normalization failed: {}", d.message))?;
    // The explicit src/policy ↔ fusion_compiler bridge must accept the IR.
    let _bridged: fusion_compiler::policy::PolicyIR = policy_ir.into();
    Ok(())
}

impl StrategyBackend for FilesystemStrategyBackend {
    fn name(&self) -> &'static str {
        "filesystem"
    }

    fn discover(&self, _ctx: &CertificationContext) -> Result<Vec<StrategyArtifact>, GateError> {
        let manifest = load_fixture_manifest(&self.loader)?;
        let entries = discover_fixtures(&manifest, FixtureKind::Strategies)?;
        let mut results = Vec::new();
        for entry in &entries {
            let full_path = self
                .loader
                .resolve(&PathBuf::from("tests/fixtures").join(&entry.path));
            results.push(self.load(&full_path)?);
        }
        Ok(results)
    }

    fn load(&self, path: &std::path::Path) -> Result<StrategyArtifact, GateError> {
        let file = self
            .loader
            .resolve_manifest_file(path, "json", "strategy")?;
        let content = self.loader.read_to_string(&file)?;
        let manifest: StrategyManifest = serde_json::from_str(&content).map_err(|e| {
            GateError::ExecutionFailed(format!("invalid strategy manifest {}: {e}", file.display()))
        })?;

        let (compiles_to_execution_graph, compile_error) =
            match verify_strategy_compilation(&manifest) {
                Ok(_) => (true, None),
                Err(e) => (false, Some(e)),
            };
        let (valid_policy, policy_error) = match verify_policy_declarations(&manifest) {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e)),
        };

        Ok(StrategyArtifact {
            name: manifest.name,
            version: manifest.version,
            pattern: manifest.pattern,
            compiles_to_execution_graph,
            valid_policy,
            compile_error,
            policy_error,
        })
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
    fn id(&self) -> GateId {
        GateId::Strategy1
    }
    fn name(&self) -> &'static str {
        "Strategy Conformance"
    }
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
            format!(
                "{failed} checks failed across {} strategies",
                artifacts.len()
            )
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
    fn name(&self) -> &'static str {
        "mock"
    }
    fn discover(&self, _ctx: &CertificationContext) -> Result<Vec<StrategyArtifact>, GateError> {
        if self.should_error {
            return Err(GateError::ExecutionFailed(
                "mock strategy backend error".into(),
            ));
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
            Box::new(MockStrategyBackend {
                artifacts: vec![],
                should_error: false,
            }),
            StrategyGateConfig {
                fixture_root: PathBuf::from("."),
            },
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
            Box::new(MockStrategyBackend {
                artifacts: vec![artifact],
                should_error: false,
            }),
            StrategyGateConfig {
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
    async fn test_strategy_gate_compilation_failure() {
        let artifact = StrategyArtifact::new(
            "single",
            semver::Version::new(0, 10, 0),
            "single/*",
            false, // Failed graph compilation
            true,
        );
        let gate = StrategyGate::new(
            Box::new(MockStrategyBackend {
                artifacts: vec![artifact],
                should_error: false,
            }),
            StrategyGateConfig {
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

    #[test]
    fn test_filesystem_strategy_backend_compiles_declared_kind_for_real() {
        let temp =
            std::env::temp_dir().join(format!("fusion_strategy_gate_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(temp.join("strategies/single")).unwrap();
        std::fs::write(
            temp.join("strategies/single/strategy.json"),
            r#"{
                "name": "real-single",
                "version": "1.2.3",
                "pattern": "real/single/*",
                "kind": "Single",
                "config": {"temperature": 0.2},
                "policy": {
                    "version": "1.0",
                    "declarations": [
                        {
                            "name": "allow-single",
                            "priority": 10,
                            "match_target": "strategy.single",
                            "effect": "allow",
                            "conditions": {},
                            "annotations": {}
                        }
                    ]
                }
            }"#,
        )
        .unwrap();

        let backend = FilesystemStrategyBackend::new(temp.clone());
        let artifact = backend.load(&temp.join("strategies/single")).unwrap();

        assert_eq!(artifact.name, "real-single");
        assert_eq!(artifact.version, semver::Version::new(1, 2, 3));
        assert_eq!(artifact.pattern, "real/single/*");
        // Both verdicts are computed, not self-declared.
        assert!(
            artifact.compiles_to_execution_graph,
            "Single must lower: {:?}",
            artifact.compile_error
        );
        assert!(
            artifact.valid_policy,
            "valid declarations must normalize+bridge: {:?}",
            artifact.policy_error
        );
        assert!(artifact.compile_error.is_none() && artifact.policy_error.is_none());

        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn test_filesystem_strategy_backend_rejects_malformed_content() {
        let temp = std::env::temp_dir().join(format!(
            "fusion_strategy_malformed_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(temp.join("strategies/single")).unwrap();
        std::fs::write(
            temp.join("strategies/single/strategy.json"),
            "this is not json {",
        )
        .unwrap();

        let backend = FilesystemStrategyBackend::new(temp.clone());
        let result = backend.load(&temp.join("strategies/single"));
        assert!(
            result.is_err(),
            "malformed manifest must not fabricate a pass"
        );

        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn test_filesystem_strategy_backend_rejects_unknown_fields() {
        let temp = std::env::temp_dir().join(format!(
            "fusion_strategy_oversized_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(temp.join("strategies/single")).unwrap();
        // An oversized fixture smuggling unknown (previously self-declared)
        // fields must be rejected outright.
        let mut json = String::from("{\n");
        for i in 0..512 {
            json.push_str(&format!("\"filler_field_{i}\": \"junk\",\n"));
        }
        json.push_str(
            "\"name\": \"single\", \"version\": \"1.0.0\", \"pattern\": \"p\", \
             \"kind\": \"Single\"\n}",
        );

        std::fs::write(temp.join("strategies/single/strategy.json"), json).unwrap();
        let backend = FilesystemStrategyBackend::new(temp.clone());
        let result = backend.load(&temp.join("strategies/single"));
        assert!(
            result.is_err(),
            "unknown fields (self-declared evidence) must fail closed"
        );

        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn test_filesystem_strategy_backend_custom_kind_fails_compilation() {
        let temp =
            std::env::temp_dir().join(format!("fusion_strategy_custom_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(temp.join("strategies/custom")).unwrap();
        // `Custom` as a bare string is not a unit variant; use the tagged form.
        std::fs::write(
            temp.join("strategies/custom/strategy.json"),
            r#"{
                "name": "unregistered-custom",
                "version": "1.0.0",
                "pattern": "custom/*",
                "kind": { "Custom": "no-such-strategy" }
            }"#,
        )
        .unwrap();

        let backend = FilesystemStrategyBackend::new(temp.clone());
        let artifact = backend.load(&temp.join("strategies/custom")).unwrap();
        assert!(
            !artifact.compiles_to_execution_graph,
            "unregistered custom strategies must fail compilation verification"
        );
        let err = artifact.compile_error.expect("failure reason recorded");
        assert!(
            err.contains("not registered"),
            "error should name the registry failure: {err}"
        );
        assert!(
            artifact.valid_policy,
            "no policy declared → nothing to violate"
        );

        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn test_filesystem_strategy_backend_invalid_policy_fails_closed() {
        let temp =
            std::env::temp_dir().join(format!("fusion_strategy_badpol_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(temp.join("strategies/bad")).unwrap();
        std::fs::write(
            temp.join("strategies/bad/strategy.json"),
            r#"{
                "name": "bad-policy",
                "version": "1.0.0",
                "pattern": "bad/*",
                "kind": "Single",
                "policy": {
                    "version": "1.0",
                    "declarations": [
                        {
                            "name": "nonsense-effect",
                            "priority": 5,
                            "match_target": "x.y",
                            "effect": "sudo",
                            "conditions": {},
                            "annotations": {}
                        }
                    ]
                }
            }"#,
        )
        .unwrap();

        let backend = FilesystemStrategyBackend::new(temp.clone());
        let artifact = backend.load(&temp.join("strategies/bad")).unwrap();
        assert!(artifact.compiles_to_execution_graph);
        assert!(
            !artifact.valid_policy,
            "invalid policy effect must fail validation"
        );
        let err = artifact.policy_error.expect("policy failure recorded");
        assert!(
            err.contains("Invalid effect") || err.contains("error"),
            "{err}"
        );

        let _ = std::fs::remove_dir_all(temp);
    }
}
