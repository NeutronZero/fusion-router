use fusion_core::NanoUSD;
use fusion_ir::{WorkflowBuilder, WorkflowMetadata, WorkflowNodeKind};
use fusion_router::ir::adapter::{uuid_for, workflow_to_types};
use fusion_router::types::{IRNodeKind, WorkflowIR as TypesWorkflowIR};

/// Phase B conformance: validates that the compilation boundary preserves all
/// semantic invariants defined in implementation_plan(1) Phase B.

/// Build a WorkflowIR containing all 9 contract kinds with full metadata.
fn build_full_ir() -> fusion_ir::WorkflowIR {
    let mut builder = WorkflowBuilder::new();
    builder = builder.metadata(WorkflowMetadata {
        policy_applied: vec!["policy_v1".into(), "policy_v2".into()],
        estimated_cost: NanoUSD::from_nanos(123_000_000),
        estimated_tokens: 5000,
    });

    let kinds = [
        ("task_node", WorkflowNodeKind::Task, Some("CodeGeneration")),
        ("tool_node", WorkflowNodeKind::Tool, Some("WebSearch")),
        ("retrieval_node", WorkflowNodeKind::Retrieval, Some("VectorSearch")),
        ("memory_node", WorkflowNodeKind::Memory, Some("MemoryStore")),
        ("review_node", WorkflowNodeKind::Review, Some("CodeReview")),
        ("judge_node", WorkflowNodeKind::Judge, Some("OutputJudge")),
        ("security_node", WorkflowNodeKind::Security, Some("SecurityAudit")),
        ("aggregation_node", WorkflowNodeKind::Aggregation, Some("Merger")),
        ("output_node", WorkflowNodeKind::Output, None),
    ];

    for (id, kind, cap) in &kinds {
        builder = builder
            .add_node(*id, *kind, *cap)
            .expect("add_node should succeed");
    }

    builder
        .sequential("task_node", "tool_node")
        .expect("edge task->tool")
        .sequential("tool_node", "retrieval_node")
        .expect("edge tool->retrieval")
        .sequential("retrieval_node", "memory_node")
        .expect("edge retrieval->memory")
        .sequential("memory_node", "review_node")
        .expect("edge memory->review")
        .sequential("review_node", "judge_node")
        .expect("edge review->judge")
        .sequential("judge_node", "security_node")
        .expect("edge judge->security")
        .sequential("security_node", "aggregation_node")
        .expect("edge security->aggregation")
        .sequential("aggregation_node", "output_node")
        .expect("edge aggregation->output")
        .build()
        .expect("build WorkflowIR")
}

// ---------------------------------------------------------------------------
// Invariant 1: Stable Identity
// ---------------------------------------------------------------------------

#[test]
fn invariant_stable_identity_deterministic_uuid_mapping() {
    let ir = build_full_ir();
    let types = workflow_to_types(&ir).expect("conversion");

    // Each node's UUID must match the deterministic uuid_for() mapping.
    for node in ir.nodes() {
        let expected_uuid = uuid_for(node.id());
        let converted = types
            .nodes
            .iter()
            .find(|n| n.id == expected_uuid)
            .unwrap_or_else(|| panic!("node {} missing after conversion", node.id()));
        assert_eq!(converted.id, expected_uuid);
    }

    // Calling uuid_for with the same input must always produce the same output.
    let a = uuid_for("task_node");
    let b = uuid_for("task_node");
    assert_eq!(a, b, "uuid_for must be deterministic");
    assert_ne!(uuid_for("task_node"), uuid_for("tool_node"));
}

// ---------------------------------------------------------------------------
// Invariant 2: Semantic Capabilities Preserved
// ---------------------------------------------------------------------------

#[test]
fn invariant_semantic_capabilities_preserved() {
    let ir = build_full_ir();
    let types = workflow_to_types(&ir).expect("conversion");

    for node in ir.nodes() {
        let expected_uuid = uuid_for(node.id());
        let converted = types
            .nodes
            .iter()
            .find(|n| n.id == expected_uuid)
            .unwrap();

        match node.capability() {
            Some(expected_cap) => {
                let config_cap = converted
                    .config
                    .get("capability")
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| {
                        panic!(
                            "node {} should have capability in config",
                            node.id()
                        )
                    });
                assert_eq!(
                    config_cap, expected_cap,
                    "capability mismatch on node {}",
                    node.id()
                );
            }
            None => {
                assert!(
                    converted.config.get("capability").is_none(),
                    "node {} should not have capability in config",
                    node.id()
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Invariant 3: Policy Versions Preserved
// ---------------------------------------------------------------------------

#[test]
fn invariant_policy_versions_preserved() {
    let ir = build_full_ir();
    let types = workflow_to_types(&ir).expect("conversion");

    assert_eq!(
        types.metadata.policy_applied,
        vec!["policy_v1".to_string(), "policy_v2".to_string()],
        "policy_applied must be preserved across compilation boundary"
    );
    assert_eq!(
        types.metadata.estimated_cost, fusion_router::types::NanoUSD::from_nanos(123_000_000),
        "estimated_cost must be preserved"
    );
    assert_eq!(
        types.metadata.estimated_tokens, 5000,
        "estimated_tokens must be preserved"
    );
}

// ---------------------------------------------------------------------------
// Invariant 4: Semantic Kind Mapping (all 9 kinds)
// ---------------------------------------------------------------------------

#[test]
fn invariant_semantic_kind_mapping_all_9_kinds() {
    let ir = build_full_ir();
    let types = workflow_to_types(&ir).expect("conversion");

    let expected_mappings: Vec<(&str, WorkflowNodeKind, IRNodeKind)> = vec![
        ("task_node", WorkflowNodeKind::Task, IRNodeKind::Generate),
        ("tool_node", WorkflowNodeKind::Tool, IRNodeKind::Generate),
        (
            "retrieval_node",
            WorkflowNodeKind::Retrieval,
            IRNodeKind::Generate,
        ),
        ("memory_node", WorkflowNodeKind::Memory, IRNodeKind::Generate),
        ("review_node", WorkflowNodeKind::Review, IRNodeKind::Review),
        ("judge_node", WorkflowNodeKind::Judge, IRNodeKind::Judge),
        (
            "security_node",
            WorkflowNodeKind::Security,
            IRNodeKind::Gate,
        ),
        (
            "aggregation_node",
            WorkflowNodeKind::Aggregation,
            IRNodeKind::Join,
        ),
        (
            "output_node",
            WorkflowNodeKind::Output,
            IRNodeKind::Transform,
        ),
    ];

    for (id, fusion_kind, expected_ir_kind) in expected_mappings {
        let uuid = uuid_for(id);
        let node = types
            .nodes
            .iter()
            .find(|n| n.id == uuid)
            .unwrap_or_else(|| panic!("node {id} missing"));

        assert_eq!(
            node.kind, expected_ir_kind,
            "IR kind mismatch for {id}: fusion-ir {fusion_kind:?} -> expected {expected_ir_kind:?}, got {:?}",
            node.kind
        );

        // Verify semantic_kind in config matches the Debug output of the fusion-ir kind.
        let semantic_kind = node
            .config
            .get("semantic_kind")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("semantic_kind missing for {id}"));
        assert_eq!(
            semantic_kind,
            format!("{:?}", fusion_kind),
            "semantic_kind in config must match fusion-ir Debug format"
        );
    }
}

// ---------------------------------------------------------------------------
// Invariant 5: Config Passthrough
// ---------------------------------------------------------------------------

#[test]
fn invariant_config_passthrough_preserves_arbitrary_keys() {
    use std::collections::BTreeMap;

    let mut config = BTreeMap::new();
    config.insert(
        "custom_key".to_string(),
        serde_json::json!({"nested": "value"}),
    );
    config.insert("priority".to_string(), serde_json::json!(42));

    let ir = WorkflowBuilder::new()
        .metadata(WorkflowMetadata::default())
        .add_node("task_a", WorkflowNodeKind::Task, Some("CodeGen"))
        .unwrap()
        .with_config("task_a", config)
        .unwrap()
        .build()
        .unwrap();

    let types = workflow_to_types(&ir).expect("conversion");

    let uuid = uuid_for("task_a");
    let node = types.nodes.iter().find(|n| n.id == uuid).unwrap();

    // Arbitrary config keys must survive the boundary.
    assert_eq!(
        node.config.get("custom_key"),
        Some(&serde_json::json!({"nested": "value"}))
    );
    assert_eq!(
        node.config.get("priority"),
        Some(&serde_json::json!(42))
    );

    // Capability must also be present.
    assert_eq!(
        node.config.get("capability").and_then(|v| v.as_str()),
        Some("CodeGen")
    );
    // Semantic kind must also be present.
    assert_eq!(
        node.config.get("semantic_kind").and_then(|v| v.as_str()),
        Some("Task")
    );
}

// ---------------------------------------------------------------------------
// Invariant 6: Edge Preservation
// ---------------------------------------------------------------------------

#[test]
fn invariant_edges_preserved_with_deterministic_uuids() {
    let ir = build_full_ir();
    let types = workflow_to_types(&ir).expect("conversion");

    // The IR has 8 sequential edges (task->tool->retrieval->...->output).
    assert_eq!(types.edges.len(), 8, "expected 8 sequential edges");

    let expected_edges: Vec<(&str, &str)> = vec![
        ("task_node", "tool_node"),
        ("tool_node", "retrieval_node"),
        ("retrieval_node", "memory_node"),
        ("memory_node", "review_node"),
        ("review_node", "judge_node"),
        ("judge_node", "security_node"),
        ("security_node", "aggregation_node"),
        ("aggregation_node", "output_node"),
    ];

    for (i, (from_id, to_id)) in expected_edges.iter().enumerate() {
        let edge = &types.edges[i];
        assert_eq!(edge.from, uuid_for(from_id), "edge[{i}] from mismatch");
        assert_eq!(edge.to, uuid_for(to_id), "edge[{i}] to mismatch");
    }
}

// ---------------------------------------------------------------------------
// Invariant 7: Plan ID Matches Workflow ID
// ---------------------------------------------------------------------------

#[test]
fn invariant_plan_id_matches_workflow_id() {
    let ir = build_full_ir();
    let types = workflow_to_types(&ir).expect("conversion");
    assert_eq!(types.plan_id, ir.workflow_id());
}

// ---------------------------------------------------------------------------
// Invariant 8: Node Count Integrity
// ---------------------------------------------------------------------------

#[test]
fn invariant_node_count_integrity() {
    let ir = build_full_ir();
    let types = workflow_to_types(&ir).expect("conversion");
    assert_eq!(
        types.nodes.len(),
        ir.nodes().len(),
        "node count must be preserved exactly"
    );
}

// ---------------------------------------------------------------------------
// Invariant 9: Minimal single-node IR
// ---------------------------------------------------------------------------

#[test]
fn invariant_single_node_ir_conversion() {
    let ir = WorkflowBuilder::new()
        .metadata(WorkflowMetadata::default())
        .add_node("solo", WorkflowNodeKind::Output, None)
        .unwrap()
        .build()
        .unwrap();

    let types = workflow_to_types(&ir).expect("conversion");
    assert_eq!(types.nodes.len(), 1);
    assert_eq!(types.edges.len(), 0);
    assert_eq!(types.metadata.policy_applied, Vec::<String>::new());

    let uuid = uuid_for("solo");
    let node = types.nodes.iter().find(|n| n.id == uuid).unwrap();
    assert_eq!(node.kind, IRNodeKind::Transform);
    assert_eq!(
        node.config.get("semantic_kind").and_then(|v| v.as_str()),
        Some("Output")
    );
}
