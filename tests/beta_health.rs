use fusion_infrastructure::{HealthLevel, PlatformHealthEngine, RecoveryEngine};

#[tokio::test]
async fn test_beta_platform_health_and_recovery_journey() {
    // 1. Health Engine Evaluation
    let health_engine = PlatformHealthEngine::new();
    let readiness = health_engine.evaluate_all();

    assert!(readiness.readiness_score_pct >= 95.0);
    assert_eq!(readiness.domain_scores.len(), 9);
    assert!(readiness.domain_scores.contains_key("Compiler"));
    assert!(readiness.domain_scores.contains_key("Providers"));
    assert!(readiness.domain_scores.contains_key("Storage"));
    assert!(readiness.domain_scores.contains_key("Security"));

    // 2. Diagnostic Report Structure Verification
    assert!(!readiness.diagnostics.is_empty());
    let diag = &readiness.diagnostics[0];
    assert_eq!(diag.status, HealthLevel::Healthy);
    assert!(!diag.suggested_fix.is_empty());

    // 3. Automated Recovery Engine Test
    let recovery_engine = RecoveryEngine::new();
    let res = recovery_engine.attempt_recovery("Providers").expect("Attempt recovery");
    assert!(res.contains("Re-tested and reconnected"));
}
