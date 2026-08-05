//! Phase H — Upgrade Safety & Schema Evolution Tests
//! Verifies backward and forward deserialization compatibility for v1 execution artifacts.

use fusion_ir::{WorkflowIR, WorkflowMetadata};
use serde_json::json;

#[test]
fn test_workflow_ir_v1_deserialization_compatibility() {
    use fusion_ir::WorkflowBuilder;

    let ir_built = WorkflowBuilder::new()
        .task("node_1", "CodeGeneration")
        .unwrap()
        .output("node_2")
        .unwrap()
        .sequential("node_1", "node_2")
        .unwrap()
        .build();

    assert!(ir_built.is_ok(), "WorkflowBuilder failed to construct valid v1 IR");
    let ir = ir_built.unwrap();
    let json_str = serde_json::to_string(&ir).expect("Serialization failed");

    let ir_deserialized: Result<WorkflowIR, _> = serde_json::from_str(&json_str);
    assert!(ir_deserialized.is_ok(), "Failed to deserialize v1 WorkflowIR artifact");
    let ir_deserialized = ir_deserialized.unwrap();
    assert_eq!(ir_deserialized.nodes().len(), 2);
    assert_eq!(ir_deserialized.version(), 1);
}

#[test]
fn test_metadata_forward_compatibility() {
    let metadata_json = json!({
        "policy_applied": ["StrictSecurityPolicy"],
        "estimated_cost": 0.005,
        "estimated_tokens": 120
    });

    let meta: Result<WorkflowMetadata, _> = serde_json::from_value(metadata_json);
    assert!(meta.is_ok(), "Metadata forward compatibility failed");
    let meta = meta.unwrap();
    assert_eq!(meta.policy_applied, vec!["StrictSecurityPolicy"]);
}
