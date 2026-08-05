use std::collections::HashMap;
use uuid::Uuid;

use fusion_router::compiler::context::CompilationContext;
use fusion_router::compiler::ir::StrategyIR;
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
        subgraph: None,
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
            Box::new(ReflectionStrategy::default()),
        ],
    };

    let ctx = CompilationContext::new();
    let ir = StrategyIR::Chain {
        stages: vec![StrategyIR::Single, StrategyIR::Reflection { max_cycles: 3 }],
    };
    let pg = strategy.lower(&ir, &ctx).unwrap();
    let node = make_node();
    let eg = pg.to_execution_graph(
        node.strategy.clone(),
        &node.retry_policy,
        &node.fallback,
        &node.config,
    );

    assert!(eg.nodes.len() >= 2, "Chain should produce at least 2 nodes");
    assert!(!eg.edges.is_empty(), "Chain should produce at least 1 edge connecting stages");

    let gen_count = eg.nodes.iter().filter(|n| is_gen(&n.kind)).count();
    let review_count = eg.nodes.iter().filter(|n| is_review(&n.kind)).count();
    assert!(gen_count >= 1, "Chain should include at least 1 Generate node");
    assert!(review_count >= 1, "Chain should include at least 1 Review node");
}

#[test]
fn test_react_strategy_produces_loop() {
    let strategy = ReActStrategy::default();

    let ctx = CompilationContext::new();
    let pg = strategy.lower(&StrategyIR::ReAct { max_iterations: 10 }, &ctx).unwrap();
    let node = make_node();
    let eg = pg.to_execution_graph(
        node.strategy.clone(),
        &node.retry_policy,
        &node.fallback,
        &node.config,
    );

    assert_eq!(eg.nodes.len(), 1, "ReAct should produce exactly 1 node (FeedbackLoop)");
    let has_loop = eg.nodes.iter().any(|n| is_loop(&n.kind));
    assert!(has_loop, "ReAct should include a Loop control node");
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

    let ctx = CompilationContext::new();
    let ir = StrategyIR::Debate {
        roles: vec![
            fusion_router::compiler::ir::DebateRole {
                name: "Defender".into(),
                model: "gpt-4".into(),
                stance: "Defend".into(),
            },
            fusion_router::compiler::ir::DebateRole {
                name: "Critic".into(),
                model: "gpt-4".into(),
                stance: "Critique".into(),
            },
        ],
    };
    let pg = strategy.lower(&ir, &ctx).unwrap();
    let node = make_node();
    let eg = pg.to_execution_graph(
        node.strategy.clone(),
        &node.retry_policy,
        &node.fallback,
        &node.config,
    );

    assert!(eg.nodes.len() >= 3, "Debate should produce at least 3 nodes (2 debaters + 1 reducer)");
}

#[test]
fn test_chain_strategy_single_stage_passthrough() {
    let strategy = ChainStrategy {
        stages: vec![Box::new(SingleStrategy)],
    };

    let ctx = CompilationContext::new();
    let ir = StrategyIR::Chain {
        stages: vec![StrategyIR::Single],
    };
    let pg = strategy.lower(&ir, &ctx).unwrap();
    let node = make_node();
    let eg = pg.to_execution_graph(
        node.strategy.clone(),
        &node.retry_policy,
        &node.fallback,
        &node.config,
    );

    assert_eq!(eg.nodes.len(), 1, "Single-stage chain should produce exactly 1 node");
}

#[test]
fn test_react_strategy_custom_max_iterations() {
    let strategy = ReActStrategy { max_iterations: 5, tool_registry: None };

    let ctx = CompilationContext::new();
    let pg = strategy.lower(&StrategyIR::ReAct { max_iterations: 5 }, &ctx).unwrap();

    let loop_node = &pg.nodes[0];
    assert!(matches!(loop_node.kind, fusion_router::compiler::ir::PrimitiveNodeKind::FeedbackLoop { max_iterations: 5 }));
}
