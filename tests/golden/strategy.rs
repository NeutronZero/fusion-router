use std::collections::HashMap;
use uuid::Uuid;

use fusion_router::strategies::Strategy;
use fusion_router::strategies::chain::ChainStrategy;
use fusion_router::strategies::debate::DebateStrategy;
use fusion_router::strategies::react::ReActStrategy;
use fusion_router::strategies::reflection::ReflectionStrategy;
use fusion_router::strategies::single::SingleStrategy;
use fusion_router::types::{
    ExecutionNode, ExecutionNodeKind, RetryPolicy, StrategyKind,
};

fn make_node() -> ExecutionNode {
    ExecutionNode {
        id: Uuid::nil(),
        kind: ExecutionNodeKind::LLMGenerate,
        strategy: StrategyKind::Single,
        model: "test-model".into(),
        retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
        fallback: None,
        config: HashMap::new(),
    }
}

fn is_gen(kind: &ExecutionNodeKind) -> bool {
    matches!(kind, ExecutionNodeKind::LLMGenerate)
}

fn is_review(kind: &ExecutionNodeKind) -> bool {
    matches!(kind, ExecutionNodeKind::LLMReview)
}

fn is_loop(kind: &ExecutionNodeKind) -> bool {
    matches!(kind, ExecutionNodeKind::Loop)
}

#[test]
fn test_chain_strategy_produces_pipeline() {
    let strategy = ChainStrategy {
        stages: vec![
            Box::new(SingleStrategy),
            Box::new(ReflectionStrategy),
        ],
    };

    let sub = strategy.apply(&make_node());

    assert!(sub.nodes.len() >= 2, "Chain should produce at least 2 nodes");
    assert!(!sub.edges.is_empty(), "Chain should produce at least 1 edge connecting stages");
    assert_ne!(sub.entry_node_id, sub.exit_node_id, "Entry and exit should differ in a multi-stage chain");

    let gen_count = sub.nodes.iter().filter(|n| is_gen(&n.kind)).count();
    let review_count = sub.nodes.iter().filter(|n| is_review(&n.kind)).count();
    assert!(gen_count >= 1, "Chain should include at least 1 Generate node");
    assert!(review_count >= 1, "Chain should include at least 1 Review node");
}

#[test]
fn test_react_strategy_produces_loop() {
    let strategy = ReActStrategy::default();

    let sub = strategy.apply(&make_node());

    assert_eq!(sub.nodes.len(), 2, "ReAct should produce exactly 2 nodes (Loop + Generate)");
    assert_eq!(sub.edges.len(), 2, "ReAct should produce 2 edges (forward + loop-back)");

    let has_loop = sub.nodes.iter().any(|n| is_loop(&n.kind));
    assert!(has_loop, "ReAct should include a Loop control node");

    let has_loop_back = sub.edges.iter().any(|e| e.condition.as_deref() == Some("loop"));
    assert!(has_loop_back, "ReAct should have a loop-back edge");

    let entry_is_loop = sub.nodes.iter().any(|n| n.id == sub.entry_node_id && is_loop(&n.kind));
    assert!(entry_is_loop, "ReAct entry should be the Loop node");
}

#[test]
fn test_debate_strategy_produces_parallel_judge() {
    let strategy = DebateStrategy {
        debaters: vec![
            Box::new(SingleStrategy),
            Box::new(SingleStrategy),
        ],
        judge: Box::new(SingleStrategy),
    };

    let sub = strategy.apply(&make_node());

    assert!(sub.nodes.len() >= 3, "Debate should produce at least 3 nodes (2 debaters + 1 judge)");

    let edges_to_judge = sub.edges.iter().filter(|e| e.to == sub.exit_node_id).count();
    assert!(edges_to_judge >= 2, "Debate should have at least 2 edges feeding into judge");

    assert_ne!(sub.entry_node_id, sub.exit_node_id, "Entry and exit should differ");
}

#[test]
fn test_chain_strategy_single_stage_passthrough() {
    let strategy = ChainStrategy {
        stages: vec![Box::new(SingleStrategy)],
    };

    let sub = strategy.apply(&make_node());

    assert_eq!(sub.nodes.len(), 1, "Single-stage chain should produce exactly 1 node");
    assert_eq!(sub.edges.len(), 0, "Single-stage chain should have no edges");
    assert_eq!(sub.entry_node_id, sub.exit_node_id, "Entry and exit should be same for single stage");
}

#[test]
fn test_react_strategy_custom_max_iterations() {
    let strategy = ReActStrategy { max_iterations: 5, tool_registry: None };

    let sub = strategy.apply(&make_node());

    let loop_node = sub.nodes.iter().find(|n| is_loop(&n.kind)).unwrap();
    let max_iter = loop_node.config.get("max_iterations").and_then(|v| v.as_u64());
    assert_eq!(max_iter, Some(5), "Loop node should have config max_iterations = 5");
}
