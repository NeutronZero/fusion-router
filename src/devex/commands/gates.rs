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
