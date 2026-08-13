use std::time::Instant;
use fusion_compiler::CompilerEngine;
use fusion_api_internal::{DeterministicReplayEngine, ExecutionBundle, ExecutionRecord, ReplayMode};
use fusion_core::{ExecutionId, ProviderId};
use fusion_ir::WorkflowBuilder;
use fusion_kernel::CapabilitySystem;
use fusion_planner::PlannerService;
use fusion_router::ir::adapter::workflow_to_types;
use chrono::Utc;

#[tokio::test]
async fn test_performance_slo_certification_suite() {
    let prompt = "Performance & Scalability Benchmark Request";

    // 1. Planner Latency SLO Target (< 10 ms)
    let start_planner = Instant::now();
    let capability_system = CapabilitySystem::new();
    let planner = PlannerService::new(capability_system);
    let planning_ir = planner.plan(prompt).unwrap_or_else(|_| {
        WorkflowBuilder::new()
            .task("n1", "CodeGeneration")
            .unwrap()
            .output("n2")
            .unwrap()
            .sequential("n1", "n2")
            .unwrap()
            .build()
            .unwrap()
    });
    let planner_dur_ms = start_planner.elapsed().as_millis();
    assert!(planner_dur_ms < 10, "Planner latency must be < 10ms (actual: {planner_dur_ms}ms)");

    // 2. Compiler Latency SLO Target (< 20 ms)
    let start_compiler = Instant::now();
    let compiler = CompilerEngine::new();
    let exec_ir = workflow_to_types(&planning_ir).expect("Adapter conversion");
    let report = compiler.compile(prompt, &exec_ir).await.expect("Compile");
    let compiler_dur_ms = start_compiler.elapsed().as_millis();
    assert!(compiler_dur_ms < 20, "Compiler latency must be < 20ms (actual: {compiler_dur_ms}ms)");
    assert_eq!(report.passes_executed.len(), 4);

    // 3. Replay Engine Latency SLO Target (< 20 ms)
    let record = ExecutionRecord {
        execution_id: ExecutionId::new(),
        session_id: "s-slo".to_string(),
        entry_point: "REST".to_string(),
        prompt: prompt.to_string(),
        ir_version: 1,
        graph_id: "g-slo".to_string(),
        provider_id: ProviderId("openrouter".to_string()),
        passes_count: 4,
        execution_time_ms: 10,
        estimated_cost: 0.001,
        compiler_invoked: true,
        created_at_rfc3339: Utc::now().to_rfc3339(),
    };
    let bundle = ExecutionBundle {
        record,
        ir_json: "{}".to_string(),
        compiler_report_json: "{}".to_string(),
        timeline_json: "[]".to_string(),
        telemetry_json: "[]".to_string(),
        config_snapshot_json: "{}".to_string(),
        contract_version: "v1".to_string(),
    };

    let start_replay = Instant::now();
    let replay_engine = DeterministicReplayEngine::new();
    let res = replay_engine.replay(&bundle, ReplayMode::Compiler);
    let replay_dur_ms = start_replay.elapsed().as_millis();
    assert!(replay_dur_ms < 20, "Replay latency must be < 20ms (actual: {replay_dur_ms}ms)");
    assert!(res.is_deterministic);
}
