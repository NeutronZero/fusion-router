use std::time::Duration;

use super::{Parallelism, StreamingMode, Strategy, StrategyDescriptor};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{
    BarrierFailurePolicy, PrimitiveGraph, PrimitiveNode, PrimitiveNodeKind, ReducerMode, StrategyIR,
};
use crate::types::{
    ArtifactKind, RetryPolicy,
};

const DEFAULT_CONSENSUS_COUNT: u32 = 3;

pub struct ConsensusStrategy {
    pub count: u32,
}

impl Default for ConsensusStrategy {
    fn default() -> Self {
        Self { count: DEFAULT_CONSENSUS_COUNT }
    }
}

impl Strategy for ConsensusStrategy {
    fn descriptor(&self) -> StrategyDescriptor {
        StrategyDescriptor {
            name: "Consensus".into(),
            parallelism: Parallelism::Fixed(self.count),
            requires_barrier: true,
            supports_streaming: StreamingMode::IncrementalArtifacts,
            retry_policy: RetryPolicy {
                max_retries: 2,
                backoff_ms: 1000,
            },
            expected_outputs: vec![ArtifactKind::Consensus],
        }
    }

    fn lower(&self, ir: &StrategyIR, ctx: &CompilationContext) -> Result<PrimitiveGraph, CompilerDiagnostic> {
        let count = match ir {
            StrategyIR::Consensus { count } => *count,
            _ => self.count,
        };

        if count < 2 {
            return Err(CompilerDiagnostic::error(
                "E0101",
                "Consensus strategy requires at least 2 parallel execution nodes",
            ));
        }

        let mut graph = PrimitiveGraph::new("consensus_graph");

        // FanOut node
        graph.add_node(PrimitiveNode {
            id: "fanout_1".into(),
            kind: PrimitiveNodeKind::FanOut { count },
            artifact_kind: None,
        });

        // Parallel generator nodes
        let model = ctx.available_models.first().cloned().unwrap_or_else(|| "default".into());
        for i in 1..=count {
            let gen_id = format!("gen_{}", i);
            graph.add_node(PrimitiveNode {
                id: gen_id.clone(),
                kind: PrimitiveNodeKind::LLMGenerate {
                    model: model.clone(),
                    role: Some(format!("consensus_member_{}", i)),
                },
                artifact_kind: Some("Consensus".into()),
            });

            graph.add_edge("fanout_1", gen_id.clone(), None);
            graph.add_edge(gen_id, "barrier_1", None);
        }

        // Barrier node
        graph.add_node(PrimitiveNode {
            id: "barrier_1".into(),
            kind: PrimitiveNodeKind::Barrier {
                min_completion: 1.0,
                timeout: Duration::from_secs(60),
                on_failure: BarrierFailurePolicy::Continue,
            },
            artifact_kind: None,
        });

        // Reducer node
        graph.add_node(PrimitiveNode {
            id: "reducer_1".into(),
            kind: PrimitiveNodeKind::Reducer {
                mode: ReducerMode::Consensus,
                model: model.clone(),
            },
            artifact_kind: Some("Consensus".into()),
        });

        graph.add_edge("barrier_1", "reducer_1", None);

        Ok(graph)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::StrategyIR;

    #[test]
    fn test_consensus_lowering() {
        let strategy = ConsensusStrategy::default();
        let ctx = CompilationContext::new();
        let graph = strategy.lower(&StrategyIR::Consensus { count: 3 }, &ctx).unwrap();

        // 1 FanOut + 3 LLMGenerate + 1 Barrier + 1 Reducer = 6 nodes
        assert_eq!(graph.nodes.len(), 6);
        assert!(matches!(graph.nodes[0].kind, PrimitiveNodeKind::FanOut { count: 3 }));
    }
}
