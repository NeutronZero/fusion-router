use std::time::Duration;
use uuid::Uuid;

use super::{Parallelism, StreamingMode, Strategy, StrategyDescriptor};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{
    BarrierFailurePolicy, PrimitiveGraph, PrimitiveNode, PrimitiveNodeKind, ReducerMode, StrategyIR,
};
use crate::types::{
    ArtifactKind, ExecutionEdge, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph, RetryPolicy, StrategyKind,
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
            name: "Consensus",
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

    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        let mut gen_ids = Vec::new();

        for _ in 0..self.count {
            let gen_id = Uuid::new_v4();
            nodes.push(ExecutionNode {
                id: gen_id,
                kind: ExecutionNodeKind::LLMGenerate,
                strategy: StrategyKind::Single,
                model: node.model.clone(),
                retry_policy: node.retry_policy.clone(),
                fallback: node.fallback.clone(),
                config: node.config.clone(),
            });
            gen_ids.push(gen_id);
        }

        let judge_id = Uuid::new_v4();
        nodes.push(ExecutionNode {
            id: judge_id,
            kind: ExecutionNodeKind::LLMJudge,
            strategy: StrategyKind::Consensus,
            model: node.model.clone(),
            retry_policy: RetryPolicy {
                max_retries: 1,
                backoff_ms: 500,
            },
            fallback: node.fallback.clone(),
            config: Default::default(),
        });

        for gen_id in &gen_ids {
            edges.push(ExecutionEdge {
                from: *gen_id,
                to: judge_id,
                condition: None,
            });
        }

        let entry_node_id = gen_ids[0];

        ExecutionSubgraph {
            nodes,
            edges,
            entry_node_id,
            exit_node_id: judge_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ExecutionNode, ExecutionNodeKind, RetryPolicy, StrategyKind};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_test_node() -> ExecutionNode {
        ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: "gpt-4".to_string(),
            retry_policy: RetryPolicy { max_retries: 3, backoff_ms: 1000 },
            fallback: None,
            config: HashMap::new(),
        }
    }

    #[test]
    fn test_consensus_default_count() {
        let strategy = ConsensusStrategy::default();
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.nodes.len(), 4);
    }

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
