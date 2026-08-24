use fusion_router::compiler::context::CompilationContext;
use fusion_router::compiler::ir::{PrimitiveNodeKind, StrategyIR};
use fusion_router::strategies::consensus::ConsensusStrategy;
use fusion_router::strategies::Strategy;

#[test]
fn test_fanout_node_matches_consensus_count() {
    let strategy = ConsensusStrategy::default();
    let ctx = CompilationContext::new();
    let ir = StrategyIR::Consensus {
        count: 5,
        members: vec![],
    };

    let graph = strategy.lower(&ir, &ctx).unwrap();
    let fanout = graph
        .nodes
        .iter()
        .find(|n| matches!(n.kind, PrimitiveNodeKind::FanOut { .. }));
    assert!(fanout.is_some());
    if let Some(n) = fanout {
        assert_eq!(n.kind, PrimitiveNodeKind::FanOut { count: 5 });
    }
}
