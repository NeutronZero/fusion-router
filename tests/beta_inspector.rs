use fusion_compiler::CompilerEngine;
use fusion_ir::WorkflowBuilder;

#[tokio::test]
async fn test_beta_compiler_inspector_journey() {
    let engine = CompilerEngine::new();
    let ir = WorkflowBuilder::new()
        .task("n1", "CodeGeneration")
        .expect("task n1")
        .output("n2")
        .expect("output n2")
        .sequential("n1", "n2")
        .expect("seq n1->n2")
        .build()
        .expect("build ir");

    let report = engine.compile("Build AST Parser", &ir, false).await.expect("Compile");

    // 1. Tab 1: Summary Validation
    assert_eq!(report.intent, "Build AST Parser");
    assert_eq!(report.compilation_time_ms, 2);

    // 2. Tab 2: Route Analysis & Provider Candidate Comparison Matrix
    assert_eq!(report.provider_comparison.len(), 3);
    assert_eq!(report.provider_comparison[0].provider_name, "openrouter");
    assert_eq!(report.provider_comparison[0].status, "Selected");
    assert_eq!(report.provider_comparison[2].status, "Filtered");
    assert!(report.provider_comparison[2].reason.contains("Missing vision"));

    // 3. Tab 3: Compiler Pass Explorer & Pass Diffs
    assert_eq!(report.pass_diffs.len(), 9);
    assert_eq!(report.pass_diffs[0].pass_name, "Validation");
    assert_eq!(report.pass_diffs[8].pass_name, "Scheduling Hints");

    // 4. Multi-dimensional Score Verification
    let explain_scores = report.route_scores;
    assert_eq!(explain_scores.len(), 3);
    assert!(explain_scores[0].capability_score >= 0.9);
}
