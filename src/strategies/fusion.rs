use std::time::Duration;

use super::{Parallelism, StreamingMode, Strategy, StrategyDescriptor};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{
    BarrierFailurePolicy, PrimitiveGraph, PrimitiveNode, PrimitiveNodeKind, ReducerMode, StrategyIR,
};
use crate::types::{
    ArtifactKind, ExecutionEdge, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph, RetryPolicy, StrategyKind,
};
use uuid::Uuid;

pub struct FusionStrategy {
    pub sub_strategies: Vec<Box<dyn Strategy>>,
}

impl Strategy for FusionStrategy {
    fn descriptor(&self) -> StrategyDescriptor {
        StrategyDescriptor {
            name: "Fusion",
            parallelism: Parallelism::Unlimited,
            requires_barrier: true,
            supports_streaming: StreamingMode::IncrementalArtifacts,
            retry_policy: RetryPolicy {
                max_retries: 2,
                backoff_ms: 1000,
            },
            expected_outputs: vec![ArtifactKind::Generic],
        }
    }

    fn lower(&self, _ir: &StrategyIR, ctx: &CompilationContext) -> Result<PrimitiveGraph, CompilerDiagnostic> {
        let count = self.sub_strategies.len() as u32;
        if count < 1 {
            return Err(CompilerDiagnostic::error(
                "E0103",
                "Fusion strategy requires at least 1 sub-strategy",
            ));
        }

        let mut graph = PrimitiveGraph::new("fusion_graph");
        let model = ctx.available_models.first().cloned().unwrap_or_else(|| "default".into());

        if count == 1 {
            graph.add_node(PrimitiveNode {
                id: "fusion_single".into(),
                kind: PrimitiveNodeKind::LLMGenerate { model, role: None },
                artifact_kind: Some("Generic".into()),
            });
            return Ok(graph);
        }

        graph.add_node(PrimitiveNode {
            id: "fanout_fusion".into(),
            kind: PrimitiveNodeKind::FanOut { count },
            artifact_kind: None,
        });

        for i in 0..count {
            let gen_id = format!("fusion_gen_{}", i + 1);
            graph.add_node(PrimitiveNode {
                id: gen_id.clone(),
                kind: PrimitiveNodeKind::LLMGenerate {
                    model: model.clone(),
                    role: Some(format!("fusion_member_{}", i + 1)),
                },
                artifact_kind: Some("Generic".into()),
            });
            graph.add_edge("fanout_fusion", gen_id.clone(), None);
            graph.add_edge(gen_id, "barrier_fusion", None);
        }

        graph.add_node(PrimitiveNode {
            id: "barrier_fusion".into(),
            kind: PrimitiveNodeKind::Barrier {
                min_completion: 1.0,
                timeout: Duration::from_secs(120),
                on_failure: BarrierFailurePolicy::Continue,
            },
            artifact_kind: None,
        });

        graph.add_node(PrimitiveNode {
            id: "reducer_fusion".into(),
            kind: PrimitiveNodeKind::Reducer {
                mode: ReducerMode::Merge,
                model,
            },
            artifact_kind: Some("Generic".into()),
        });

        graph.add_edge("barrier_fusion", "reducer_fusion", None);

        Ok(graph)
    }

    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        if self.sub_strategies.is_empty() {
            return ExecutionSubgraph {
                nodes: vec![node.clone()],
                edges: vec![],
                entry_node_id: node.id,
                exit_node_id: node.id,
            };
        }

        let mut all_nodes = Vec::new();
        let mut all_edges = Vec::new();
        let mut exits = Vec::new();
        let mut entry_id = None;

        for sub in &self.sub_strategies {
            let subgraph = sub.apply(node);
            if entry_id.is_none() {
                entry_id = Some(subgraph.entry_node_id);
            }
            exits.push(subgraph.exit_node_id);
            all_nodes.extend(subgraph.nodes);
            all_edges.extend(subgraph.edges);
        }

        if exits.len() > 1 {
            let judge_id = Uuid::new_v4();
            all_nodes.push(ExecutionNode {
                id: judge_id,
                kind: ExecutionNodeKind::LLMJudge,
                strategy: StrategyKind::Fusion,
                model: node.model.clone(),
                retry_policy: RetryPolicy {
                    max_retries: 1,
                    backoff_ms: 500,
                },
                fallback: node.fallback.clone(),
                config: Default::default(),
            });
            for exit_id in &exits {
                all_edges.push(ExecutionEdge {
                    from: *exit_id,
                    to: judge_id,
                    condition: None,
                });
            }
            ExecutionSubgraph {
                nodes: all_nodes,
                edges: all_edges,
                entry_node_id: entry_id.unwrap_or(node.id),
                exit_node_id: judge_id,
            }
        } else {
            ExecutionSubgraph {
                nodes: all_nodes,
                edges: all_edges,
                entry_node_id: entry_id.unwrap_or(node.id),
                exit_node_id: exits[0],
            }
        }
    }
}
