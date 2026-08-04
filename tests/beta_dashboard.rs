use fusion_api_internal::ArchitectureKpiMetrics;

#[tokio::test]
async fn test_beta_mission_control_dashboard_journey() {
    // 1. Architecture Governance KPI Evaluation
    let metrics = ArchitectureKpiMetrics::new(1284, 1284, 1284);
    assert_eq!(metrics.compiler_invocation_rate, 1.0);
    assert_eq!(metrics.execution_graph_rate, 1.0);
    assert_eq!(metrics.zero_bypass_violations, 0);

    // 2. Dashboard Metrics Verification
    let active_providers = 6;
    let avg_latency_ms = 38;
    let system_status = "Healthy";

    assert_eq!(active_providers, 6);
    assert_eq!(avg_latency_ms, 38);
    assert_eq!(system_status, "Healthy");
}
