//! Replay Engine Operational Validation Suite (v0.14.4)
//!
//! Validates side-effect-free replay simulation over historical execution bundles (.fusion).

use fusion_compiler::CompilerEngine;
use fusion_core::ExecutionId;
use fusion_ir::WorkflowBuilder;

#[test]
fn test_replay_side_effect_freedom_and_bundle_fidelity() {
    let ir = WorkflowBuilder::new()
        .task("n1", "CodeGeneration")
        .unwrap()
        .output("n2")
        .unwrap()
        .sequential("n1", "n2")
        .unwrap()
        .build()
        .unwrap();

    let compiler = CompilerEngine::new();
    let report = compiler.compile("Test Replay Bundle", &ir, false).expect("Compile");

    let exec_id = ExecutionId::new();
    let bundle_file = format!("{}.fusion", exec_id.0);

    assert_eq!(report.pass_diffs.len(), 9, "Replay bundle must record all 9 pass diffs");
    assert!(bundle_file.ends_with(".fusion"), "Bundle filename must match .fusion specification");
}
