use fusion_router::compiler::context::CompilationContext;
use fusion_router::compiler::ir::{StrategyIR, DebateRole, PrimitiveGraph};
use fusion_router::strategies::debate::DebateStrategy;
use fusion_router::strategies::single::SingleStrategy;
use fusion_router::strategies::consensus::ConsensusStrategy;
use fusion_router::strategies::Strategy;

#[test]
fn test_primitive_graph_hash_and_mermaid_export() {
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

    let graph = strategy.lower(&ir, &ctx).unwrap();
    let hash = graph.compute_hash();
    assert_ne!(hash, 0);

    let mermaid = graph.to_mermaid();
    assert!(mermaid.contains("graph TD"));
    assert!(mermaid.contains("fanout_debate"));
    assert!(mermaid.contains("debater_1"));
    assert!(mermaid.contains("debater_2"));
    assert!(mermaid.contains("barrier_debate"));
    assert!(mermaid.contains("reducer_debate"));

    let dot = graph.to_dot();
    assert!(dot.contains("digraph PrimitiveGraph"));
}

#[test]
fn test_golden_ir_snapshots() {
    let ctx = CompilationContext::new();

    // Single strategy golden snapshot
    let single_graph = SingleStrategy.lower(&StrategyIR::Single, &ctx).unwrap();
    let single_snapshot: PrimitiveGraph = serde_json::from_str(
        include_str!("../../golden_ir/single.json")
    ).expect("valid single.json snapshot");
    assert_eq!(single_graph, single_snapshot);

    // Consensus strategy golden snapshot
    let consensus_graph = ConsensusStrategy::default()
        .lower(&StrategyIR::Consensus { count: 3 }, &ctx)
        .unwrap();
    let consensus_snapshot: PrimitiveGraph = serde_json::from_str(
        include_str!("../../golden_ir/consensus.json")
    ).expect("valid consensus.json snapshot");
    assert_eq!(consensus_graph, consensus_snapshot);

    // Debate strategy golden snapshot
    let debate_strategy = DebateStrategy {
        debaters: vec![],
        judge: Box::new(SingleStrategy),
    };
    let debate_graph = debate_strategy.lower(
        &StrategyIR::Debate {
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
        },
        &ctx,
    ).unwrap();
    let debate_snapshot: PrimitiveGraph = serde_json::from_str(
        include_str!("../../golden_ir/debate.json")
    ).expect("valid debate.json snapshot");
    assert_eq!(debate_graph, debate_snapshot);
}
