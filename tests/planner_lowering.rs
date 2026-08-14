use fusion_router::intent::{intent_to_workflow, Budget, Constraints, IntentKind, NormalizedIntent};
use fusion_router::ir::{WorkflowEdgeKind, WorkflowNodeKind, WorkflowIR, WORKFLOW_IR_VERSION};
use uuid::Uuid;

fn sample_intent() -> NormalizedIntent {
    NormalizedIntent {
        intent_id: Uuid::new_v4(),
        goal: "implement the payments endpoint".into(),
        kind: IntentKind::Code,
        constraints: Constraints {
            max_latency_ms: Some(5_000),
            ..Constraints::default()
        },
        budget: Budget {
            max_cost_usd: Some(0.05),
            max_tokens: Some(4096),
            max_execution_ms: None,
        },
        session_id: None,
    }
}

#[test]
fn intent_lowers_to_canonical_workflow() {
    let ir: WorkflowIR = intent_to_workflow(&sample_intent()).unwrap();
    assert_eq!(ir.version(), WORKFLOW_IR_VERSION);
    assert_eq!(ir.nodes().len(), 2);
    assert_eq!(ir.nodes()[0].kind(), WorkflowNodeKind::Task);
    assert_eq!(
        ir.nodes()[0].config().get("goal"),
        Some(&serde_json::Value::String("implement the payments endpoint".into()))
    );
    assert_eq!(ir.edges().len(), 1);
    assert_eq!(ir.edges()[0].kind(), WorkflowEdgeKind::Sequential);
    assert_eq!(ir.metadata().estimated_cost, fusion_core::NanoUSD::from_nanos(50_000_000));
    assert_eq!(ir.metadata().estimated_tokens, 4096);
    assert!(ir.validate().is_empty());
}
