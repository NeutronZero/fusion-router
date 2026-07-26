use fusion_router::compiler::context::CompilationContext;
use fusion_router::compiler::ir::{StrategyIR, DebateRole};
use fusion_router::strategies::debate::DebateStrategy;
use fusion_router::strategies::single::SingleStrategy;
use fusion_router::strategies::Strategy;

fn main() {
    println!("=== FusionRouter Strategy SDK: Structured Debate Compiler Pipeline ===");

    let debate_strategy = DebateStrategy {
        debaters: vec![],
        judge: Box::new(SingleStrategy),
    };

    let ctx = CompilationContext::new();

    let ir = StrategyIR::Debate {
        roles: vec![
            DebateRole {
                name: "Defender".into(),
                model: "claude-opus-4".into(),
                stance: "Defend FusionRouter 2-layer IR compiler architecture".into(),
            },
            DebateRole {
                name: "Critic".into(),
                model: "gpt-4o".into(),
                stance: "Critique compilation latency and layer overhead".into(),
            },
            DebateRole {
                name: "SecurityAuditor".into(),
                model: "claude-sonnet-4".into(),
                stance: "Audit artifact schema versioning and ABI security".into(),
            },
        ],
    };

    let descriptor = debate_strategy.descriptor();
    println!("Strategy Descriptor: name={}, parallelism={:?}, requires_barrier={}", 
        descriptor.name, descriptor.parallelism, descriptor.requires_barrier);

    println!("\n--- Compiling StrategyIR -> PrimitiveGraph ---");
    let graph = debate_strategy.lower(&ir, &ctx).expect("successful debate lowering");

    println!("PrimitiveGraph Hash: 0x{:x}", graph.compute_hash());
    println!("Primitive Nodes Count: {}", graph.nodes.len());
    println!("Primitive Edges Count: {}", graph.edges.len());

    println!("\n--- Compiled Mermaid Architecture Diagram ---");
    println!("{}", graph.to_mermaid());

    println!("--- Compiled Graphviz DOT ---");
    println!("{}", graph.to_dot());
}
