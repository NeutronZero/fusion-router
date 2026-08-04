use fusion_api_internal::ExecutionBundle;
use fusion_kernel::{CapabilityRegistry, ExecutionProfile};

#[test]
fn test_af004_contract_compatibility_suite() {
    // 1. WorkflowIR v1 & CapabilityRegistry Contract
    let registry = CapabilityRegistry::new();
    assert!(registry.supports("Vision"));
    assert!(registry.supports("JSON"));
    assert!(registry.supports("ToolCalling"));
    assert!(registry.supports("MCP"));

    // 2. Execution Profiles Contract
    let profiles = vec![
        ExecutionProfile::Fast,
        ExecutionProfile::Balanced,
        ExecutionProfile::Cheap,
        ExecutionProfile::Coding,
        ExecutionProfile::Research,
        ExecutionProfile::Vision,
        ExecutionProfile::Reasoning,
        ExecutionProfile::Creative,
        ExecutionProfile::Offline,
    ];
    assert_eq!(profiles.len(), 9);

    // 3. ExecutionBundle v1 Schema Compatibility
    let json_schema = r#"{
        "record": {
            "execution_id": "00000000-0000-0000-0000-000000000000",
            "session_id": "s1",
            "entry_point": "REST",
            "prompt": "test",
            "ir_version": 1,
            "graph_id": "g1",
            "provider_id": "openrouter",
            "passes_count": 9,
            "execution_time_ms": 10,
            "estimated_cost": 0.001,
            "compiler_invoked": true,
            "created_at_rfc3339": "2026-08-04T20:24:00Z"
        },
        "ir_json": "{}",
        "compiler_report_json": "{}",
        "timeline_json": "[]",
        "telemetry_json": "[]",
        "config_snapshot_json": "{}",
        "contract_version": "v1"
    }"#;

    let bundle = ExecutionBundle::import_bundle(json_schema).expect("Deserialization MUST succeed for v1 contract");
    assert_eq!(bundle.contract_version, "v1");
}
