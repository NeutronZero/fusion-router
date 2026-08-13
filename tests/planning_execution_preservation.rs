use fusion_ir::{WorkflowBuilder, WorkflowMetadata, WorkflowNodeKind};
use fusion_router::ir::adapter::{uuid_for, workflow_to_types};

#[test]
fn test_preservation_conformance_all_9_kinds() {
    let mut builder = WorkflowBuilder::new();
    builder = builder.metadata(WorkflowMetadata {
        policy_applied: vec!["policy_v1".into()],
        estimated_cost: 0.05,
        estimated_tokens: 1000,
    });

    let kinds = [
        ("node_task", WorkflowNodeKind::Task, Some("CodeGeneration")),
        ("node_tool", WorkflowNodeKind::Tool, Some("WebSearch")),
        ("node_retrieval", WorkflowNodeKind::Retrieval, Some("VectorSearch")),
        ("node_memory", WorkflowNodeKind::Memory, Some("MemoryStore")),
        ("node_review", WorkflowNodeKind::Review, Some("CodeReview")),
        ("node_judge", WorkflowNodeKind::Judge, Some("OutputJudge")),
        ("node_security", WorkflowNodeKind::Security, Some("SecurityAudit")),
        ("node_aggregation", WorkflowNodeKind::Aggregation, Some("Merger")),
        ("node_output", WorkflowNodeKind::Output, None),
    ];

    for (id, kind, cap) in kinds.iter() {
        builder = builder.add_node(*id, kind.clone(), cap.map(|s| s.to_string())).expect("Add node");
    }

    let ir = builder.build().expect("Build WorkflowIR");
    let converted = workflow_to_types(&ir).expect("Convert workflow_to_types");

    assert_eq!(converted.nodes.len(), 9);
    assert_eq!(converted.metadata.policy_applied, vec!["policy_v1".to_string()]);
    assert_eq!(converted.metadata.estimated_cost, 0.05);

    for (id, kind, cap) in kinds.iter() {
        let expected_uuid = uuid_for(id);
        let node = converted.nodes.iter().find(|n| n.id == expected_uuid)
            .expect(&format!("Node {id} missing"));

        let semantic_kind = node.config.get("semantic_kind").and_then(|v| v.as_str())
            .expect("semantic_kind missing in config");
        assert_eq!(semantic_kind, format!("{:?}", kind));

        if let Some(expected_cap) = cap {
            let config_cap = node.config.get("capability").and_then(|v| v.as_str())
                .expect("capability missing in config");
            assert_eq!(config_cap, *expected_cap);
        }
    }
}
