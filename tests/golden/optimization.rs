use fusion_router::compiler::ir::{PrimitiveGraph, PrimitiveNode, PrimitiveNodeKind, BarrierFailurePolicy};
use fusion_router::compiler::optimization::{DeadNodeEliminationPass, FanOutConsolidationPass, OptimizationPass, OptimizationPipeline};

fn connected_graph() -> PrimitiveGraph {
    let mut g = PrimitiveGraph::new("connected");
    g.add_node(PrimitiveNode {
        id: "entry".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "a".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "exit".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_edge("entry", "a", None);
    g.add_edge("a", "exit", None);
    g
}

fn graph_with_disconnected_node() -> PrimitiveGraph {
    let mut g = PrimitiveGraph::new("disconnected");
    g.add_node(PrimitiveNode {
        id: "entry".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "orphan".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_edge("entry", "entry", None);
    g
}

fn graph_with_unused_subtree() -> PrimitiveGraph {
    let mut g = PrimitiveGraph::new("unused_subtree");
    g.add_node(PrimitiveNode {
        id: "entry".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "a".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "exit".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "x".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "y".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_edge("entry", "a", None);
    g.add_edge("a", "exit", None);
    g.add_edge("x", "y", None);
    g
}

fn graph_with_barrier() -> PrimitiveGraph {
    let mut g = PrimitiveGraph::new("with_barrier");
    g.add_node(PrimitiveNode {
        id: "entry".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "barrier".into(),
        kind: PrimitiveNodeKind::Barrier { min_completion: 1.0, timeout: std::time::Duration::from_secs(30), on_failure: fusion_router::compiler::ir::BarrierFailurePolicy::Abort },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "a".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_edge("entry", "barrier", None);
    g.add_edge("barrier", "a", None);
    g
}

fn graph_with_reducer() -> PrimitiveGraph {
    let mut g = PrimitiveGraph::new("with_reducer");
    g.add_node(PrimitiveNode {
        id: "entry".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "reducer".into(),
        kind: PrimitiveNodeKind::Reducer { mode: fusion_router::compiler::ir::ReducerMode::Consensus, model: "gpt-4".into() },
        artifact_kind: None,
    });
    g.add_edge("entry", "reducer", None);
    g
}

fn graph_with_feedback_loop() -> PrimitiveGraph {
    let mut g = PrimitiveGraph::new("with_feedback");
    g.add_node(PrimitiveNode {
        id: "entry".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "loop".into(),
        kind: PrimitiveNodeKind::FeedbackLoop { max_iterations: 3 },
        artifact_kind: None,
    });
    g.add_edge("entry", "loop", None);
    g
}

fn graph_single_node() -> PrimitiveGraph {
    let mut g = PrimitiveGraph::new("single");
    g.add_node(PrimitiveNode {
        id: "only".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g
}

#[test]
fn test_removes_disconnected_node() {
    let pass = DeadNodeEliminationPass::new();
    let graph = graph_with_disconnected_node();
    let result = pass.optimize(graph).unwrap();
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.nodes[0].id, "entry");
}

#[test]
fn test_removes_unused_subtree() {
    let pass = DeadNodeEliminationPass::new();
    let graph = graph_with_unused_subtree();
    let result = pass.optimize(graph).unwrap();
    assert_eq!(result.nodes.len(), 3);
    for node in &result.nodes {
        assert!(node.id == "entry" || node.id == "a" || node.id == "exit", "unexpected node {}", node.id);
    }
}

#[test]
fn test_keeps_barrier() {
    let pass = DeadNodeEliminationPass::new();
    let graph = graph_with_barrier();
    let result = pass.optimize(graph).unwrap();
    assert_eq!(result.nodes.len(), 3);
    assert!(result.nodes.iter().any(|n| n.id == "barrier"));
}

#[test]
fn test_keeps_reducer() {
    let pass = DeadNodeEliminationPass::new();
    let graph = graph_with_reducer();
    let result = pass.optimize(graph).unwrap();
    assert_eq!(result.nodes.len(), 2);
    assert!(result.nodes.iter().any(|n| n.id == "reducer"));
}

#[test]
fn test_keeps_feedback_loop() {
    let pass = DeadNodeEliminationPass::new();
    let graph = graph_with_feedback_loop();
    let result = pass.optimize(graph).unwrap();
    assert_eq!(result.nodes.len(), 2);
    assert!(result.nodes.iter().any(|n| n.id == "loop"));
}

#[test]
fn test_all_nodes_connected_no_change() {
    let pass = DeadNodeEliminationPass::new();
    let graph = connected_graph();
    let result = pass.optimize(graph).unwrap();
    assert_eq!(result.nodes.len(), 3);
}

#[test]
fn test_single_node_preserved() {
    let pass = DeadNodeEliminationPass::new();
    let graph = graph_single_node();
    let result = pass.optimize(graph).unwrap();
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.nodes[0].id, "only");
}

#[test]
fn test_empty_graph() {
    let pass = DeadNodeEliminationPass::new();
    let graph = PrimitiveGraph::new("empty");
    let result = pass.optimize(graph).unwrap();
    assert_eq!(result.nodes.len(), 0);
}

#[test]
fn test_pipeline_integration() {
    use fusion_router::compiler::optimization::OptimizationPipeline;
    let mut pipeline = OptimizationPipeline::new();
    pipeline.add_pass(Box::new(DeadNodeEliminationPass::new()));
    let graph = graph_with_disconnected_node();
    let result = pipeline.run(graph).unwrap();
    assert_eq!(result.nodes.len(), 1);
}

#[test]
fn test_goal_returns_graph_simplification() {
    let pass = DeadNodeEliminationPass::new();
    assert_eq!(pass.goal(), fusion_router::compiler::optimization::OptimizationGoal::GraphSimplification);
}

fn fanout_graph_adjacent() -> PrimitiveGraph {
    let mut g = PrimitiveGraph::new("adjacent_fanouts");
    g.add_node(PrimitiveNode {
        id: "entry".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "fo1".into(),
        kind: PrimitiveNodeKind::FanOut { count: 2 },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "fo2".into(),
        kind: PrimitiveNodeKind::FanOut { count: 4 },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "a".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_edge("entry", "fo1", None);
    g.add_edge("fo1", "fo2", None);
    g.add_edge("fo2", "a", None);
    g
}

fn fanout_graph_single_consumer() -> PrimitiveGraph {
    let mut g = PrimitiveGraph::new("single_consumer");
    g.add_node(PrimitiveNode {
        id: "entry".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "fo".into(),
        kind: PrimitiveNodeKind::FanOut { count: 1 },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "a".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_edge("entry", "fo", None);
    g.add_edge("fo", "a", None);
    g
}

fn fanout_graph_no_consolidation() -> PrimitiveGraph {
    let mut g = PrimitiveGraph::new("no_change");
    g.add_node(PrimitiveNode {
        id: "entry".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "fo".into(),
        kind: PrimitiveNodeKind::FanOut { count: 3 },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "a".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "b".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "c".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_edge("entry", "fo", None);
    g.add_edge("fo", "a", None);
    g.add_edge("fo", "b", None);
    g.add_edge("fo", "c", None);
    g
}

fn fanout_graph_with_barrier() -> PrimitiveGraph {
    let mut g = PrimitiveGraph::new("with_barrier_pass");
    g.add_node(PrimitiveNode {
        id: "entry".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "fo".into(),
        kind: PrimitiveNodeKind::FanOut { count: 2 },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "a".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "b".into(),
        kind: PrimitiveNodeKind::LLMGenerate { model: "gpt-4".into(), role: None },
        artifact_kind: None,
    });
    g.add_node(PrimitiveNode {
        id: "barrier".into(),
        kind: PrimitiveNodeKind::Barrier { min_completion: 1.0, timeout: std::time::Duration::from_secs(30), on_failure: BarrierFailurePolicy::Abort },
        artifact_kind: None,
    });
    g.add_edge("entry", "fo", None);
    g.add_edge("fo", "a", None);
    g.add_edge("fo", "b", None);
    g.add_edge("a", "barrier", None);
    g.add_edge("b", "barrier", None);
    g
}

#[test]
fn test_fanout_consolidation_adjacent_merged() {
    let pass = FanOutConsolidationPass::new();
    let graph = fanout_graph_adjacent();
    let result = pass.optimize(graph).unwrap();
    let fanouts: Vec<_> = result.nodes.iter().filter(|n| matches!(n.kind, PrimitiveNodeKind::FanOut { .. })).collect();
    assert_eq!(fanouts.len(), 1, "adjacent FanOuts should merge into one");
    if let PrimitiveNodeKind::FanOut { count } = &fanouts[0].kind {
        assert_eq!(count, &4, "merged FanOut should have max count");
    }
}

#[test]
fn test_fanout_consolidation_single_consumer_eliminated() {
    let pass = FanOutConsolidationPass::new();
    let graph = fanout_graph_single_consumer();
    let result = pass.optimize(graph).unwrap();
    let fanouts: Vec<_> = result.nodes.iter().filter(|n| matches!(n.kind, PrimitiveNodeKind::FanOut { .. })).collect();
    assert_eq!(fanouts.len(), 0, "single-consumer FanOut should be eliminated");
    assert_eq!(result.nodes.len(), 2, "entry and a remain");
}

#[test]
fn test_fanout_consolidation_multi_consumer_unchanged() {
    let pass = FanOutConsolidationPass::new();
    let graph = fanout_graph_no_consolidation();
    let result = pass.optimize(graph).unwrap();
    let fanouts: Vec<_> = result.nodes.iter().filter(|n| matches!(n.kind, PrimitiveNodeKind::FanOut { .. })).collect();
    assert_eq!(fanouts.len(), 1, "multi-consumer FanOut preserved");
    assert_eq!(result.nodes.len(), 5);
}

#[test]
fn test_fanout_consolidation_barrier_preserved() {
    let pass = FanOutConsolidationPass::new();
    let graph = fanout_graph_with_barrier();
    let result = pass.optimize(graph).unwrap();
    let barriers: Vec<_> = result.nodes.iter().filter(|n| matches!(n.kind, PrimitiveNodeKind::Barrier { .. })).collect();
    assert_eq!(barriers.len(), 1, "Barrier with multiple inputs preserved");
    assert_eq!(result.nodes.len(), 5);
}

#[test]
fn test_fanout_consolidation_empty_graph() {
    let pass = FanOutConsolidationPass::new();
    let graph = PrimitiveGraph::new("empty");
    let result = pass.optimize(graph).unwrap();
    assert_eq!(result.nodes.len(), 0);
}

#[test]
fn test_fanout_consolidation_pipeline_composition() {
    let mut pipeline = OptimizationPipeline::new();
    pipeline.add_pass(Box::new(DeadNodeEliminationPass::new()));
    pipeline.add_pass(Box::new(FanOutConsolidationPass::new()));
    let graph = fanout_graph_adjacent();
    let result = pipeline.run(graph).unwrap();
    let fanouts: Vec<_> = result.nodes.iter().filter(|n| matches!(n.kind, PrimitiveNodeKind::FanOut { .. })).collect();
    assert_eq!(fanouts.len(), 1);
}
