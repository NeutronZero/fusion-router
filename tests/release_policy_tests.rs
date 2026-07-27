use std::path::PathBuf;
use fusion_router::release::bootstrap::build_default_runner;
use fusion_router::release::evaluator::{EvaluationContext, PolicyEvaluator, ReleaseDecision};
use fusion_router::release::gate::GateContext;
use fusion_router::release::policy::{load_policy_from_yaml, PolicyDefinition, ReleaseEnvironment};
use fusion_router::release::waiver::{load_waivers_from_yaml, WaiverSet};

#[tokio::test]
async fn test_policy_evaluation_end_to_end_production() {
    let workspace_root = PathBuf::from(".");
    let runner = build_default_runner(workspace_root.clone(), "HEAD");
    let context = GateContext {
        workspace_root: workspace_root.clone(),
        baseline_version: None,
    };

    let results = runner.run_all(&context).await;
    let policy = load_policy_from_yaml(&workspace_root.join("tests/fixtures/policy.yaml"))
        .unwrap_or_else(|_| PolicyDefinition::default_policy());
    let waivers = load_waivers_from_yaml(&workspace_root.join("tests/fixtures/waivers.yaml"))
        .unwrap_or_default();

    let eval_ctx = EvaluationContext::new(ReleaseEnvironment::Production, policy, waivers);
    let eval = PolicyEvaluator::evaluate(&eval_ctx, &results);

    // Assert that policy evaluation runs cleanly and returns a valid decision
    assert!(
        matches!(
            eval.decision,
            ReleaseDecision::Approved | ReleaseDecision::ApprovedWithWaivers | ReleaseDecision::Blocked
        )
    );
    assert_eq!(eval.environment, ReleaseEnvironment::Production);
}

#[tokio::test]
async fn test_policy_evaluation_end_to_end_staging() {
    let workspace_root = PathBuf::from(".");
    let runner = build_default_runner(workspace_root.clone(), "HEAD");
    let context = GateContext {
        workspace_root: workspace_root.clone(),
        baseline_version: None,
    };

    let results = runner.run_all(&context).await;
    let policy = PolicyDefinition::default_policy();
    let eval_ctx = EvaluationContext::new(ReleaseEnvironment::Staging, policy, WaiverSet::default());
    let eval = PolicyEvaluator::evaluate(&eval_ctx, &results);

    assert_eq!(eval.environment, ReleaseEnvironment::Staging);
}
