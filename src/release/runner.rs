use super::gate::{GateContext, GateExecution, GateId, GateResult, ReleaseGate};
use std::time::Duration;

#[derive(Default)]
pub struct GateRunner {
    gates: Vec<Box<dyn ReleaseGate>>,
}

impl GateRunner {
    pub fn new() -> Self {
        Self { gates: Vec::new() }
    }

    pub fn register(&mut self, gate: Box<dyn ReleaseGate>) {
        self.gates.push(gate);
    }

    #[allow(dead_code)]
    pub fn gates(&self) -> &[Box<dyn ReleaseGate>] {
        &self.gates
    }

    pub async fn run_all(&self, context: &GateContext) -> Vec<GateExecution> {
        let mut results = Vec::with_capacity(self.gates.len());
        for gate in &self.gates {
            results.push(Self::normalize(gate.id(), gate.run(context).await));
        }
        results
    }

    pub async fn run_one(&self, id: GateId, context: &GateContext) -> Option<GateExecution> {
        for gate in &self.gates {
            if gate.id() == id {
                return Some(Self::normalize(gate.id(), gate.run(context).await));
            }
        }
        None
    }

    /// Converts a gate execution error into a failed `GateResult` carrying the
    /// gate's identity, so consumers (e.g. `PolicyEvaluator`) can classify
    /// errors as failed evidence instead of silently dropping them.
    fn normalize(id: GateId, execution: GateExecution) -> GateExecution {
        match execution {
            GateExecution::ExecutionError(error) => GateExecution::Success(GateResult {
                gate_id: id,
                passed: false,
                summary: error.to_string(),
                details: vec![],
                duration: Duration::ZERO,
            }),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::gate::{GateCategory, GateError, GateMetadata, GateResult, MockGate};
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    struct FailingGate {
        id: GateId,
    }

    #[async_trait]
    impl ReleaseGate for FailingGate {
        fn id(&self) -> GateId {
            self.id
        }

        fn name(&self) -> &str {
            "FailingGate"
        }

        fn description(&self) -> &str {
            "A gate that always fails"
        }

        fn metadata(&self) -> &GateMetadata {
            unimplemented!("not needed for test")
        }

        async fn run(&self, _context: &GateContext) -> GateExecution {
            GateExecution::ExecutionError(GateError::ExecutionFailed("something broke".into()))
        }
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

    fn test_context() -> GateContext {
        GateContext {
            workspace_root: PathBuf::from("/tmp"),
            baseline_version: None,
        }
    }

    fn success_result(id: GateId) -> GateExecution {
        GateExecution::Success(GateResult {
            gate_id: id,
            passed: true,
            summary: "OK".into(),
            details: vec![],
            duration: Duration::from_secs(1),
        })
    }

    #[tokio::test]
    async fn test_runner_run_all_returns_results() {
        let meta = GateMetadata {
            id: GateId::Sdk1,
            category: GateCategory::Compatibility,
            required: true,
            introduced: semver::Version::new(0, 10, 0),
        };
        let gate = MockGate::new(
            GateId::Sdk1,
            "SDK Compat",
            "Checks SDK compatibility",
            meta,
            success_result(GateId::Sdk1),
        );
        let mut runner = GateRunner::new();
        runner.register(Box::new(gate));

        let results = runner.run_all(&test_context()).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].passed());
    }

    #[tokio::test]
    async fn test_runner_run_one_by_id() {
        let meta = GateMetadata {
            id: GateId::Sdk1,
            category: GateCategory::Compatibility,
            required: true,
            introduced: semver::Version::new(0, 10, 0),
        };
        let gate = MockGate::new(
            GateId::Sdk1,
            "SDK Compat",
            "Checks SDK compatibility",
            meta,
            success_result(GateId::Sdk1),
        );
        let mut runner = GateRunner::new();
        runner.register(Box::new(gate));

        let result = runner.run_one(GateId::Sdk1, &test_context()).await;
        assert!(result.is_some());
        assert!(result.unwrap().passed());
    }

    #[tokio::test]
    async fn test_runner_run_one_unknown_returns_none() {
        let meta = GateMetadata {
            id: GateId::Sdk1,
            category: GateCategory::Compatibility,
            required: true,
            introduced: semver::Version::new(0, 10, 0),
        };
        let gate = MockGate::new(
            GateId::Sdk1,
            "SDK Compat",
            "Checks SDK compatibility",
            meta,
            success_result(GateId::Sdk1),
        );
        let mut runner = GateRunner::new();
        runner.register(Box::new(gate));

        let result = runner.run_one(GateId::Upgrade1, &test_context()).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_runner_execution_error_converted_to_failed_result() {
        let failing = FailingGate {
            id: GateId::Replay1,
        };
        let mut runner = GateRunner::new();
        runner.register(Box::new(failing));

        let results = runner.run_all(&test_context()).await;
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed());
        assert!(!results[0].is_error());
    }

    #[tokio::test]
    async fn test_runner_error_becomes_failed_result_with_identity() {
        let failing = FailingGate {
            id: GateId::Replay1,
        };
        let mut runner = GateRunner::new();
        runner.register(Box::new(failing));

        let results = runner.run_all(&test_context()).await;
        assert_eq!(results.len(), 1);
        match &results[0] {
            GateExecution::Success(result) => {
                assert_eq!(result.gate_id, GateId::Replay1);
                assert!(!result.passed);
                assert!(result.summary.contains("something broke"));
            }
            GateExecution::ExecutionError(_) => {
                panic!("expected execution error to be converted to a failed GateResult, got ExecutionError");
            }
        }
    }

    #[tokio::test]
    async fn test_runner_run_one_error_becomes_failed_result() {
        let failing = FailingGate {
            id: GateId::Upgrade1,
        };
        let mut runner = GateRunner::new();
        runner.register(Box::new(failing));

        let result = runner.run_one(GateId::Upgrade1, &test_context()).await;
        let execution = result.expect("gate should be found");
        match execution {
            GateExecution::Success(res) => {
                assert_eq!(res.gate_id, GateId::Upgrade1);
                assert!(!res.passed);
            }
            GateExecution::ExecutionError(_) => {
                panic!("expected error to be converted to a failed GateResult");
            }
        }
    }

    #[tokio::test]
    async fn test_runner_fifo_order() {
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
}
