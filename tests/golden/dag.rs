use std::collections::HashMap;
use uuid::Uuid;

use fusion_router::compiler::Compiler;
use fusion_router::compiler::passes::{ControlFlowValidationPass, ConstraintValidationPass, ModelResolutionPass};
use fusion_router::compiler::CompilerPass;
use fusion_router::types::{
    IRMetadata, IRNode, IRNodeKind, IREdge, StrategyKind, WorkflowIR,
};
use fusion_router::compiler::DefaultCompiler;

#[tokio::test]
async fn test_control_flow_conditional_valid() {
    let pass = ControlFlowValidationPass;
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let c = Uuid::new_v4();

    let mut cond_config = HashMap::new();
    cond_config.insert("condition".into(), serde_json::json!("is_code"));

    let mut ir = WorkflowIR {
        plan_id: Uuid::nil(),
        nodes: vec![
            IRNode { id: a, kind: IRNodeKind::Conditional, strategy: StrategyKind::Single, model: None, config: cond_config },
            IRNode { id: b, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: c, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![
            IREdge { from: a, to: b, condition: Some("true".into()) },
            IREdge { from: a, to: c, condition: Some("false".into()) },
        ],
        metadata: IRMetadata {
            policy_applied: vec!["test".into()],
            estimated_cost: 0.01,
            estimated_tokens: 500,
        },
    };
    ir.nodes[1].strategy = StrategyKind::Single;
    ir.nodes[2].strategy = StrategyKind::Single;

    let result = pass.apply(ir).await;
    assert!(result.is_ok(), "Conditional with condition edges should pass");
}

#[tokio::test]
async fn test_control_flow_conditional_no_condition_edge() {
    let pass = ControlFlowValidationPass;
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();

    let ir = WorkflowIR {
        plan_id: Uuid::nil(),
        nodes: vec![
            IRNode { id: a, kind: IRNodeKind::Conditional, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: b, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![
            IREdge { from: a, to: b, condition: None },
        ],
        metadata: IRMetadata {
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 0,
        },
    };

    let result = pass.apply(ir).await;
    assert!(result.is_err(), "Conditional without condition on any edge should fail");
}

#[tokio::test]
async fn test_control_flow_loop_valid() {
    let pass = ControlFlowValidationPass;
    let loop_node = Uuid::new_v4();
    let body = Uuid::new_v4();
    let exit = Uuid::new_v4();

    let mut loop_config = HashMap::new();
    loop_config.insert("max_iterations".into(), serde_json::json!(5));

    let ir = WorkflowIR {
        plan_id: Uuid::nil(),
        nodes: vec![
            IRNode { id: loop_node, kind: IRNodeKind::Loop, strategy: StrategyKind::Single, model: None, config: loop_config },
            IRNode { id: body, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: exit, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![
            IREdge { from: loop_node, to: body, condition: None },
            IREdge { from: body, to: loop_node, condition: Some("loop".into()) },
            IREdge { from: loop_node, to: exit, condition: Some("exit".into()) },
        ],
        metadata: IRMetadata {
            policy_applied: vec!["test".into()],
            estimated_cost: 0.01,
            estimated_tokens: 500,
        },
    };

    let result = pass.apply(ir).await;
    assert!(result.is_ok(), "Loop with max_iterations should pass");
}

#[tokio::test]
async fn test_control_flow_split_join_valid() {
    let pass = ControlFlowValidationPass;
    let split = Uuid::new_v4();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let join = Uuid::new_v4();

    let ir = WorkflowIR {
        plan_id: Uuid::nil(),
        nodes: vec![
            IRNode { id: split, kind: IRNodeKind::Split, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: a, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: b, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: join, kind: IRNodeKind::Join, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![
            IREdge { from: split, to: a, condition: None },
            IREdge { from: split, to: b, condition: None },
            IREdge { from: a, to: join, condition: None },
            IREdge { from: b, to: join, condition: None },
        ],
        metadata: IRMetadata {
            policy_applied: vec!["test".into()],
            estimated_cost: 0.02,
            estimated_tokens: 1000,
        },
    };

    let result = pass.apply(ir).await;
    assert!(result.is_ok(), "Split/Join with proper edges should pass");
}

#[tokio::test]
async fn test_control_flow_split_no_outgoing() {
    let pass = ControlFlowValidationPass;
    let split = Uuid::new_v4();

    let ir = WorkflowIR {
        plan_id: Uuid::nil(),
        nodes: vec![
            IRNode { id: split, kind: IRNodeKind::Split, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![],
        metadata: IRMetadata {
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 0,
        },
    };

    let result = pass.apply(ir).await;
    assert!(result.is_err(), "Split with no outgoing edges should fail");
}

#[tokio::test]
async fn test_control_flow_loop_no_max_iterations() {
    let pass = ControlFlowValidationPass;
    let loop_node = Uuid::new_v4();

    let ir = WorkflowIR {
        plan_id: Uuid::nil(),
        nodes: vec![
            IRNode { id: loop_node, kind: IRNodeKind::Loop, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![],
        metadata: IRMetadata {
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 0,
        },
    };

    let result = pass.apply(ir).await;
    assert!(result.is_err(), "Loop without max_iterations should fail");
}

#[tokio::test]
async fn test_control_flow_barrier_valid() {
    let pass = ControlFlowValidationPass;
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let barrier = Uuid::new_v4();
    let c = Uuid::new_v4();

    let ir = WorkflowIR {
        plan_id: Uuid::nil(),
        nodes: vec![
            IRNode { id: a, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: b, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: barrier, kind: IRNodeKind::Barrier, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: c, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![
            IREdge { from: a, to: barrier, condition: None },
            IREdge { from: b, to: barrier, condition: None },
            IREdge { from: barrier, to: c, condition: None },
        ],
        metadata: IRMetadata {
            policy_applied: vec!["test".into()],
            estimated_cost: 0.01,
            estimated_tokens: 500,
        },
    };

    let result = pass.apply(ir).await;
    assert!(result.is_ok(), "Barrier with incoming and outgoing edges should pass");
}

#[tokio::test]
async fn test_compiler_passes_handle_all_node_kinds() {
    let compiler = DefaultCompiler {
        passes: vec![
            Box::new(ConstraintValidationPass),
            Box::new(ControlFlowValidationPass),
            Box::new(ModelResolutionPass),
        ],
    };

    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let c = Uuid::new_v4();
    let d = Uuid::new_v4();

    let ir = WorkflowIR {
        plan_id: Uuid::nil(),
        nodes: vec![
            IRNode { id: a, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: b, kind: IRNodeKind::Split, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: c, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: d, kind: IRNodeKind::Join, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![
            IREdge { from: a, to: b, condition: None },
            IREdge { from: b, to: c, condition: None },
            IREdge { from: b, to: d, condition: None },
            IREdge { from: c, to: d, condition: None },
        ],
        metadata: IRMetadata {
            policy_applied: vec!["test".into()],
            estimated_cost: 0.02,
            estimated_tokens: 1000,
        },
    };

    let result = compiler.compile(ir).await;
    assert!(result.is_ok(), "Compiler should handle mixed DAG with Split/Join");

    let graph = result.unwrap();
    assert_eq!(graph.nodes.len(), 4);
    assert_eq!(graph.edges.len(), 4);
}

#[tokio::test]
async fn detect_cycle_disconnected_subgraph() {
    let pass = ControlFlowValidationPass;

    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let c = Uuid::new_v4();
    let d = Uuid::new_v4();

    let ir = WorkflowIR {
        plan_id: Uuid::nil(),
        nodes: vec![
            IRNode { id: a, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: b, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: c, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            IRNode { id: d, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
        ],
        edges: vec![
            IREdge { from: a, to: b, condition: None },
            IREdge { from: b, to: c, condition: None },
            IREdge { from: c, to: a, condition: None },
            // d is disconnected from the a→b→c cycle
        ],
        metadata: IRMetadata {
            policy_applied: vec!["test".into()],
            estimated_cost: 0.01,
            estimated_tokens: 500,
        },
    };

    let result = pass.apply(ir).await;
    assert!(
        result.is_err(),
        "Should detect cycle even in disconnected subgraph (a→b→c→a)"
    );
}
