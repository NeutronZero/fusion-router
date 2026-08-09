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

    let report = engine.compile("Build AST Parser", &ir).await.expect("Compile");

    // 1. Tab 1: Summary Validation
    assert_eq!(report.intent, "Build AST Parser");
    // compilation_time_ms is now a real measurement, not hardcoded
    assert!(report.compilation_time_ms <= 100, "compilation_time_ms should be reasonable: {}", report.compilation_time_ms);

    // 2. Tab 2: Route Analysis & Provider Candidate Comparison Matrix
    assert_eq!(report.provider_comparison.len(), 3);
    assert_eq!(report.provider_comparison[0].provider_name, "openrouter");
    // All providers have same total_score (1.0 from budget_score only),
    // so first is "Alternative" not "Selected" — tied, not uniquely best (ADR-039 D2)
    assert_eq!(report.provider_comparison[0].status, "Alternative");
    assert!(report.provider_comparison[0].reason.contains("Tied with"));

    // 3. Tab 3: Compiler Pass Explorer & Pass Diffs
    assert_eq!(report.pass_diffs.len(), 11);
    assert_eq!(report.pass_diffs[0].pass_name, "constraint_validation");
    assert_eq!(report.pass_diffs[9].pass_name, "Scheduling Hints");

    // 4. Multi-dimensional Score Verification
    let explain_scores = report.route_scores;
    assert_eq!(explain_scores.len(), 3);
    // capability_score is None (not yet wired in crates/, see ADR-039)
    assert!(explain_scores[0].capability_score.is_none());
    // budget_score is Some(1.0) from StubResourceManager
    assert_eq!(explain_scores[0].budget_score, Some(1.0));
}
