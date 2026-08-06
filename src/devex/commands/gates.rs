use std::time::Duration;

use crate::release::gate::{GateContext, GateExecution, GateId, GateResult};
use crate::release::report::GateReport;
use crate::release::runner::GateRunner;

pub fn list_gates(runner: &GateRunner) -> String {
    let gates = runner.gates();
    if gates.is_empty() {
        return "No gates registered.".to_string();
    }
    let mut output = String::new();
    output.push_str("Registered release gates:\n");
    for gate in gates {
        output.push_str(&format!(
            "  [{}] {} - {}\n",
            gate.id(),
            gate.name(),
            gate.description()
        ));
    }
    output
}

pub fn explain_gate(runner: &GateRunner, id: GateId) -> String {
    for gate in runner.gates() {
        if gate.id() == id {
            let meta = gate.metadata();
            return format!(
                "Gate: {}\n  ID: {}\n  Category: {:?}\n  Required: {}\n  Introduced: {}\n  Description: {}",
                gate.name(),
                gate.id(),
                meta.category,
                meta.required,
                meta.introduced,
                gate.description(),
            );
        }
    }
    format!("Gate '{}' not found.", id)
}

pub async fn check_gates(runner: &GateRunner, context: &GateContext) -> String {
    let gates = runner.gates();
    let executions = runner.run_all(context).await;

    let mut results = Vec::with_capacity(executions.len());
    for (gate, execution) in gates.iter().zip(executions.iter()) {
        match execution {
            GateExecution::Success(result) => results.push(result.clone()),
            GateExecution::ExecutionError(err) => results.push(GateResult {
                gate_id: gate.id(),
                passed: false,
                summary: format!("Execution error: {}", err),
                details: vec![],
                duration: Duration::ZERO,
            }),
        }
    }

    let version = context
        .baseline_version
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "0.0.0".to_string());

    let report = GateReport::new(results, version);
    report.to_text()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::gate::{GateCategory, GateMetadata, MockGate};

    fn sample_runner() -> GateRunner {
        let meta = GateMetadata {
            id: GateId::Sdk1,
            category: GateCategory::Compatibility,
            required: true,
            introduced: semver::Version::new(1, 0, 0),
        };
        let mock_gate = MockGate::new(
            GateId::Sdk1,
            "SDK Test Gate",
            "Validates SDK compatibility",
            meta,
            GateExecution::Success(GateResult {
                gate_id: GateId::Sdk1,
                passed: true,
                summary: "SDK gate passed".into(),
                details: vec![],
                duration: Duration::from_millis(10),
            }),
        );
        let mut runner = GateRunner::new();
        runner.register(Box::new(mock_gate));
        runner
    }

    #[test]
    fn test_list_gates_empty() {
        let runner = GateRunner::new();
        let output = list_gates(&runner);
        assert_eq!(output, "No gates registered.");
    }

    #[test]
    fn test_list_gates_with_entries() {
        let runner = sample_runner();
        let output = list_gates(&runner);
        assert!(output.contains("Registered release gates:"));
        assert!(output.contains("SDK Test Gate"));
    }

    #[test]
    fn test_explain_gate_found() {
        let runner = sample_runner();
        let output = explain_gate(&runner, GateId::Sdk1);
        assert!(output.contains("Gate: SDK Test Gate"));
        assert!(output.contains("Validates SDK compatibility"));
    }

    #[test]
    fn test_explain_gate_not_found() {
        let runner = sample_runner();
        let output = explain_gate(&runner, GateId::Upgrade1);
        assert!(output.contains("Gate 'UPG-1' not found."));
    }
}
