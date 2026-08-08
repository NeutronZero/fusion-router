use fusion_router::compiler::context::CompilationContext;
use fusion_router::compiler::ir::{StrategyIR, DebateRole};
use fusion_router::strategies::single::SingleStrategy;
use fusion_router::strategies::consensus::ConsensusStrategy;
use fusion_router::strategies::debate::DebateStrategy;
use fusion_router::strategies::Strategy;

#[test]
fn test_single_lowering_is_deterministic() {
    let strategy = SingleStrategy;
    let ctx = CompilationContext::new();
    let ir = StrategyIR::Single;

    let graph1 = strategy.lower(&ir, &ctx).unwrap();
    let graph2 = strategy.lower(&ir, &ctx).unwrap();

    let json1 = serde_json::to_string(&graph1).unwrap();
    let json2 = serde_json::to_string(&graph2).unwrap();

    assert_eq!(json1, json2);
}

#[test]
fn test_consensus_lowering_is_deterministic() {
    let strategy = ConsensusStrategy::default();
    let ctx = CompilationContext::new();
    let ir = StrategyIR::Consensus { count: 3, members: vec![] };

    let graph1 = strategy.lower(&ir, &ctx).unwrap();
    let graph2 = strategy.lower(&ir, &ctx).unwrap();

    let json1 = serde_json::to_string(&graph1).unwrap();
    let json2 = serde_json::to_string(&graph2).unwrap();

    assert_eq!(json1, json2);
}

#[test]
fn test_debate_lowering_is_deterministic() {
    let strategy = DebateStrategy {
        debaters: vec![],
        judge: Box::new(SingleStrategy),
    };
    let ctx = CompilationContext::new();
    let ir = StrategyIR::Debate {
        roles: vec![
            DebateRole {
                name: "Defender".into(),
                model: "claude-opus-4".into(),
                stance: "Defend".into(),
            },
            DebateRole {
                name: "Critic".into(),
                model: "gpt-4o".into(),
                stance: "Critique".into(),
            },
        ],
    };

    let graph1 = strategy.lower(&ir, &ctx).unwrap();
    let graph2 = strategy.lower(&ir, &ctx).unwrap();

    let json1 = serde_json::to_string(&graph1).unwrap();
    let json2 = serde_json::to_string(&graph2).unwrap();

    assert_eq!(json1, json2);
}
