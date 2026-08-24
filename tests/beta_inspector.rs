use fusion_compiler::CompilerEngine;
use fusion_ir::WorkflowBuilder;
use fusion_router::ir::adapter::workflow_to_types;

#[tokio::test]
async fn test_beta_compiler_inspector_journey() {
    let engine = CompilerEngine::new();
    let planning_ir = WorkflowBuilder::new()
        .task("n1", "CodeGeneration")
        .expect("task n1")
        .output("n2")
        .expect("output n2")
        .sequential("n1", "n2")
        .expect("seq n1->n2")
        .build()
        .expect("build ir");

    let exec_ir = workflow_to_types(&planning_ir).expect("Adapter conversion");
    let report = engine
        .compile("Build AST Parser", &exec_ir)
        .await
        .expect("Compile");

    // 1. Tab 1: Summary Validation
    assert_eq!(report.intent, "Build AST Parser");
    assert!(
        report.compilation_time_ms <= 100,
        "compilation_time_ms should be reasonable: {}",
        report.compilation_time_ms
    );

    // 2. Tab 2: Route Analysis & Provider Candidate Comparison Matrix
    assert_eq!(report.provider_comparison.len(), 3);
    // Phase 5: static tables differentiate — zen wins, no ties at 1.0
    assert_eq!(report.provider_comparison[0].provider_name, "zen");
    assert_eq!(report.provider_comparison[0].status, "Selected");
    assert_eq!(report.provider_comparison[1].status, "Alternative");
    assert_eq!(report.provider_comparison[2].status, "Filtered");
    assert!(
        report.provider_comparison[0].total_score > report.provider_comparison[1].total_score,
        "provider totals must be differentiated"
    );

    // 3. Tab 3: Compiler Pass Explorer & Pass Diffs (5 passes in Phase 3)
    assert_eq!(report.pass_diffs.len(), 5);
    assert_eq!(report.pass_diffs[0].pass_name, "constraint_validation");

    // 4. Multi-dimensional Score Verification
    let explain_scores = report.route_scores;
    assert_eq!(explain_scores.len(), 3);
    assert!(
        explain_scores[0].capability_score.is_some(),
        "capability score must be present (Phase 5)"
    );
    assert_eq!(explain_scores[0].budget_score, Some(1.0));
}
