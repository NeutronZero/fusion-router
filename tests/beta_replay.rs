use chrono::Utc;
use fusion_api_internal::{
    ArchitectureKpiMetrics, DeterministicReplayEngine, ExecutionBundle, ExecutionRecord, ReplayMode,
};
use fusion_core::{ExecutionId, ProviderId};

#[tokio::test]
async fn test_beta_execution_intelligence_and_replay_journey() {
    let exec_id_1 = ExecutionId::new();
    let exec_id_2 = ExecutionId::new();

    // 1. Construct ExecutionRecord
    let record1 = ExecutionRecord {
        execution_id: exec_id_1,
        session_id: "session-100".to_string(),
        entry_point: "REST_CHAT".to_string(),
        prompt: "Build compiler pass".to_string(),
        ir_version: 1,
        graph_id: "graph_001".to_string(),
        provider_id: ProviderId("openrouter".to_string()),
        passes_count: 11,
        execution_time_ms: 62,
        estimated_cost: fusion_core::NanoUSD::from_nanos(1_200_000),
        compiler_invoked: true,
        created_at_rfc3339: Utc::now().to_rfc3339(),
    };

    let bundle1 = ExecutionBundle {
        record: record1.clone(),
        ir_json: r#"{"nodes":2}"#.to_string(),
        compiler_report_json: r#"{"passes":11}"#.to_string(),
        timeline_json: r#"[{"name":"Planning"}]"#.to_string(),
        telemetry_json: r#"[]"#.to_string(),
        config_snapshot_json: r#"{"version":1}"#.to_string(),
        contract_version: "v1".to_string(),
    };

    // 2. Export & Import Round-trip Test
    let bundle_json = bundle1.export_bundle().expect("Export .fusion bundle");
    let imported_bundle = ExecutionBundle::import_bundle(&bundle_json).expect("Import .fusion bundle");
    assert_eq!(imported_bundle.record.execution_id.0, exec_id_1.0);
    assert_eq!(imported_bundle.contract_version, "v1");

    // 3. 3-Mode Deterministic Replay Test
    let replay_engine = DeterministicReplayEngine::new();

    let timeline_replay = replay_engine.replay(&imported_bundle, ReplayMode::Timeline);
    assert_eq!(timeline_replay.mode, ReplayMode::Timeline);
    assert!(timeline_replay.is_deterministic);
    assert_eq!(timeline_replay.replay_fidelity, 1.0);

    let compiler_replay = replay_engine.replay(&imported_bundle, ReplayMode::Compiler);
    assert_eq!(compiler_replay.mode, ReplayMode::Compiler);
    assert_eq!(compiler_replay.steps_replayed, 11);

    let runtime_replay = replay_engine.replay(&imported_bundle, ReplayMode::Runtime);
    assert_eq!(runtime_replay.mode, ReplayMode::Runtime);

    // 4. Execution Comparison Diff Test
    let mut record2 = record1.clone();
    record2.execution_id = exec_id_2;
    record2.provider_id = ProviderId("zen".to_string());
    record2.execution_time_ms = 84;
    record2.estimated_cost = fusion_core::NanoUSD::from_nanos(1_800_000);

    let diff = replay_engine.compare(&record1, &record2);
    assert!(diff.provider_changed);
    assert_eq!(diff.latency_delta_ms, 22);

    // 5. Replay Fidelity KPI Certification
    let metrics = ArchitectureKpiMetrics::new(100, 100, 100);
    assert_eq!(metrics.replay_fidelity_rate, 1.0);
}
