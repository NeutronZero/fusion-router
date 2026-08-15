use fusion_ir::{
    WorkflowBuilder, WorkflowEdge, WorkflowEdgeKind, WorkflowIR, WorkflowNode, WorkflowNodeKind,
    WORKFLOW_IR_VERSION,
};

#[test]
fn shim_resolves_frozen_contract_types() {
    let ir: WorkflowIR = WorkflowBuilder::new()
        .task("n1", "CodeGeneration")
        .unwrap()
        .output("n2")
        .unwrap()
        .sequential("n1", "n2")
        .unwrap()
        .build()
        .unwrap();
    let nodes: &[WorkflowNode] = ir.nodes();
    let edges: &[WorkflowEdge] = ir.edges();
    assert_eq!(ir.version(), WORKFLOW_IR_VERSION);
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].kind(), WorkflowNodeKind::Task);
    assert_eq!(nodes[0].capability(), Some("CodeGeneration"));
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].kind(), WorkflowEdgeKind::Sequential);
    assert_eq!(ir.metadata().estimated_tokens, 0);
    let json = serde_json::to_string(&ir).unwrap();
    assert!(json.contains("\"version\":1"));
}
