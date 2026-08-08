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
        let (count, members) = match ir {
            StrategyIR::Consensus { count, members } => (*count, members.clone()),
            _ => (self.count, Vec::new()),
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

        // Parallel generator nodes. Each member may name its own model
        // (e.g. a multi-model code review where zen/ and openrouter/ models
        // each review independently); the member list is cycled when the
        // member count exceeds the list length, and members without an
        // explicit model fall back to the node's model.
        let default_model = ctx.available_models.first().cloned().unwrap_or_else(|| "default".into());
        let member_pool: Vec<String> = members.into_iter().filter(|m| !m.is_empty()).collect();
        for i in 1..=count {
            let gen_id = format!("gen_{}", i);
            let model = if member_pool.is_empty() {
                default_model.clone()
            } else {
                member_pool[(i - 1) as usize % member_pool.len()].clone()
            };
            graph.add_node(PrimitiveNode {
                id: gen_id.clone(),
                kind: PrimitiveNodeKind::LLMGenerate {
                    model,
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

        // Reducer node — the judge consolidating member reviews. Prefer the
        // last explicitly-named member model (the "senior" reviewer) when
        // present, else the node model.
        let reducer_model = member_pool.last().cloned().unwrap_or(default_model);
        graph.add_node(PrimitiveNode {
            id: "reducer_1".into(),
            kind: PrimitiveNodeKind::Reducer {
                mode: ReducerMode::Consensus,
                model: reducer_model,
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
        let graph = strategy.lower(&StrategyIR::Consensus { count: 3, members: vec![] }, &ctx).unwrap();

        // 1 FanOut + 3 LLMGenerate + 1 Barrier + 1 Reducer = 6 nodes
        assert_eq!(graph.nodes.len(), 6);
        assert!(matches!(graph.nodes[0].kind, PrimitiveNodeKind::FanOut { count: 3 }));
    }

    fn member_models(graph: &PrimitiveGraph) -> Vec<&str> {
        graph
            .nodes
            .iter()
            .filter_map(|n| match &n.kind {
                PrimitiveNodeKind::LLMGenerate { model, .. } => Some(model.as_str()),
                _ => None,
            })
            .collect()
    }

    fn reducer_model(graph: &PrimitiveGraph) -> &str {
        graph
            .nodes
            .iter()
            .find_map(|n| match &n.kind {
                PrimitiveNodeKind::Reducer { model, .. } => Some(model.as_str()),
                _ => None,
            })
            .unwrap_or("")
    }

    #[test]
    fn test_consensus_members_assign_distinct_models() {
        let strategy = ConsensusStrategy::default();
        let ctx = CompilationContext::new();
        let graph = strategy
            .lower(
                &StrategyIR::Consensus {
                    count: 3,
                    members: vec![
                        "zen/model-a".into(),
                        "openrouter/model-b".into(),
                        "openrouter/model-c".into(),
                    ],
                },
                &ctx,
            )
            .unwrap();

        assert_eq!(member_models(&graph), vec!["zen/model-a", "openrouter/model-b", "openrouter/model-c"]);
        assert_eq!(reducer_model(&graph), "openrouter/model-c");
    }

    #[test]
    fn test_consensus_members_cycle_when_shorter_than_count() {
        let strategy = ConsensusStrategy::default();
        let ctx = CompilationContext::new();
        let graph = strategy
            .lower(
                &StrategyIR::Consensus {
                    count: 4,
                    members: vec!["zen/model-a".into()],
                },
                &ctx,
            )
            .unwrap();

        assert_eq!(
            member_models(&graph),
            vec!["zen/model-a", "zen/model-a", "zen/model-a", "zen/model-a"]
        );
        assert_eq!(reducer_model(&graph), "zen/model-a");
    }

    #[test]
    fn test_consensus_no_members_uses_node_model() {
        let strategy = ConsensusStrategy::default();
        let mut ctx = CompilationContext::new();
        ctx.available_models.push("node-model".into());
        let graph = strategy
            .lower(&StrategyIR::Consensus { count: 2, members: vec![] }, &ctx)
            .unwrap();

        assert_eq!(member_models(&graph), vec!["node-model", "node-model"]);
        assert_eq!(reducer_model(&graph), "node-model");
    }
}
