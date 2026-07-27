use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fusion_router::feature_gate::*;
use fusion_router::release::gate::*;
use fusion_router::release::gates::semver::{MockBackend, SemVerGate};
use fusion_router::release::report::GateReport;
use fusion_router::release::runner::GateRunner;

fn test_context() -> GateContext {
    GateContext {
        workspace_root: PathBuf::from("/tmp"),
        baseline_version: None,
    }
}

#[tokio::test]
async fn test_gate_runner_with_mock_semver_passing() {
    let gate = SemVerGate::with_backend(
        Box::new(MockBackend { should_pass: true }),
        "v0.9.0",
        PathBuf::from("/tmp"),
    );
    let mut runner = GateRunner::new();
    runner.register(Box::new(gate));

    let results = runner.run_all(&test_context()).await;
    assert_eq!(results.len(), 1);
    assert!(results[0].passed());

    let gate_results: Vec<GateResult> = results
        .into_iter()
        .filter_map(|r| match r {
            GateExecution::Success(result) => Some(result),
            _ => None,
        })
        .collect();
    let report = GateReport::new(gate_results, "1.0.0".to_string());
    assert!(report.overall);
}

#[tokio::test]
async fn test_gate_runner_with_mock_semver_failing() {
    let gate = SemVerGate::with_backend(
        Box::new(MockBackend { should_pass: false }),
        "v0.9.0",
        PathBuf::from("/tmp"),
    );
    let mut runner = GateRunner::new();
    runner.register(Box::new(gate));

    let results = runner.run_all(&test_context()).await;
    assert_eq!(results.len(), 1);
    assert!(!results[0].passed());

    let gate_results: Vec<GateResult> = results
        .into_iter()
        .filter_map(|r| match r {
            GateExecution::Success(result) => Some(result),
            _ => None,
        })
        .collect();
    let report = GateReport::new(gate_results, "1.0.0".to_string());
    assert!(!report.overall);
}

#[tokio::test]
async fn test_report_json_round_trip() {
    let gate = SemVerGate::with_backend(
        Box::new(MockBackend { should_pass: true }),
        "v0.9.0",
        PathBuf::from("/tmp"),
    );
    let mut runner = GateRunner::new();
    runner.register(Box::new(gate));

    let results = runner.run_all(&test_context()).await;
    let gate_results: Vec<GateResult> = results
        .into_iter()
        .filter_map(|r| match r {
            GateExecution::Success(result) => Some(result),
            _ => None,
        })
        .collect();
    let report = GateReport::new(gate_results, "1.0.0".to_string());

    let json = serde_json::to_string(&report).unwrap();
    let deserialized: GateReport = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.version, report.version);
    assert_eq!(deserialized.overall, report.overall);
    assert_eq!(
        deserialized.duration.as_secs_f64(),
        report.duration.as_secs_f64()
    );
    assert_eq!(deserialized.gates.len(), report.gates.len());
    assert_eq!(deserialized.gates[0].gate_id, report.gates[0].gate_id);
    assert_eq!(deserialized.gates[0].passed, report.gates[0].passed);
}

struct OrderedGate {
    id: GateId,
    counter: Arc<AtomicUsize>,
    expected: usize,
}

#[async_trait]
impl ReleaseGate for OrderedGate {
    fn id(&self) -> GateId {
        self.id
    }

    fn name(&self) -> &str {
        "OrderedGate"
    }

    fn description(&self) -> &str {
        "Gate that records execution order"
    }

    fn metadata(&self) -> &GateMetadata {
        unimplemented!("not needed for test")
    }

    async fn run(&self, _context: &GateContext) -> GateExecution {
        let prev = self.counter.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            prev, self.expected,
            "Gate {:?} executed out of order",
            self.id
        );
        GateExecution::Success(GateResult {
            gate_id: self.id,
            passed: true,
            summary: format!("Gate {:?} executed at position {}", self.id, prev),
            details: vec![],
            duration: Duration::from_secs(0),
        })
    }
}

#[tokio::test]
async fn test_fifo_execution_order() {
    let counter = Arc::new(AtomicUsize::new(0));

    let gate_a = OrderedGate {
        id: GateId::Sdk1,
        counter: Arc::clone(&counter),
        expected: 0,
    };
    let gate_b = OrderedGate {
        id: GateId::Replay1,
        counter: Arc::clone(&counter),
        expected: 1,
    };
    let gate_c = OrderedGate {
        id: GateId::Upgrade1,
        counter: Arc::clone(&counter),
        expected: 2,
    };

    let mut runner = GateRunner::new();
    runner.register(Box::new(gate_a));
    runner.register(Box::new(gate_b));
    runner.register(Box::new(gate_c));

    let results = runner.run_all(&test_context()).await;
    assert_eq!(results.len(), 3);
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[test]
fn test_feature_registry_integration() {
    let definitions: &[FeatureDefinition] = &[FeatureDefinition {
        id: FeatureFlag::Streaming,
        introduced: "0.11.0",
        removed: None,
        stability: Stability::Stable,
        default_enabled: true,
        description: "Streaming execution support",
    }];

    let mut registry = FeatureRegistry::new(definitions);

    assert!(registry.is_enabled(FeatureFlag::Streaming));
    assert!(registry.is_effectively_enabled(FeatureFlag::Streaming));

    let mut config = HashMap::new();
    config.insert(
        "streaming".to_string(),
        FeatureConfig { enabled: false },
    );
    registry.apply_config(&config);

    assert!(!registry.is_enabled(FeatureFlag::Streaming));
    assert!(!registry.is_effectively_enabled(FeatureFlag::Streaming));
}

#[test]
fn test_bootstrap_registers_all_gates() {
    use fusion_router::release::bootstrap::build_default_runner;
    let runner = build_default_runner(PathBuf::from("."), "HEAD");
    let gate_ids: Vec<GateId> = runner.gates().iter().map(|g| g.id()).collect();
    assert_eq!(gate_ids.len(), 8, "expected exactly 8 gates registered");
    assert_eq!(gate_ids[0], GateId::Sdk1);
    assert_eq!(gate_ids[1], GateId::Replay1);
    assert_eq!(gate_ids[2], GateId::Upgrade1);
    assert_eq!(gate_ids[3], GateId::Determinism1);
    assert_eq!(gate_ids[4], GateId::Plugin1);
    assert_eq!(gate_ids[5], GateId::Strategy1);
    assert_eq!(gate_ids[6], GateId::Provider1);
    assert_eq!(gate_ids[7], GateId::Connector1);
}
