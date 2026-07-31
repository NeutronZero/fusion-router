use fusion_router::ir::{WorkflowIR, WorkflowNode, WorkflowNodeKind, WorkflowEdge, WorkflowEdgeKind, WorkflowMetadata, WORKFLOW_IR_VERSION};

#[test]
fn shim_resolves_frozen_contract_types() {
    let ir: WorkflowIR = WorkflowIR {
        version: WORKFLOW_IR_VERSION,
        workflow_id: uuid::Uuid::new_v4(),
        nodes: vec![WorkflowNode {
            id: "n1".into(),
            kind: WorkflowNodeKind::Task,
            capability: None,
            config: Default::default(),
        }],
        edges: vec![WorkflowEdge {
            from: "n1".into(),
            to: "n1".into(),
            kind: WorkflowEdgeKind::Loop,
            condition: None,
        }],
        metadata: WorkflowMetadata::default(),
    };
    let json = serde_json::to_string(&ir).unwrap();
    assert!(json.contains("\"version\":1"));
}
