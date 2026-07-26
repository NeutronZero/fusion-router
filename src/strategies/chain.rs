use uuid::Uuid;

use super::{Parallelism, StreamingMode, Strategy, StrategyDescriptor};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{PrimitiveGraph, StrategyIR};
use crate::types::{ArtifactKind, ExecutionEdge, ExecutionNode, ExecutionSubgraph, RetryPolicy};

pub struct ChainStrategy {
    pub stages: Vec<Box<dyn Strategy>>,
}

impl Strategy for ChainStrategy {
    fn descriptor(&self) -> StrategyDescriptor {
        StrategyDescriptor {
            name: "Chain",
            parallelism: Parallelism::Sequential,
            requires_barrier: false,
            supports_streaming: StreamingMode::Full,
            retry_policy: RetryPolicy {
                max_retries: 2,
                backoff_ms: 1000,
            },
            expected_outputs: vec![ArtifactKind::Generic],
        }
    }

    fn lower(&self, ir: &StrategyIR, ctx: &CompilationContext) -> Result<PrimitiveGraph, CompilerDiagnostic> {
        let mut combined_graph = PrimitiveGraph::new("chain_graph");

        let stages = match ir {
            StrategyIR::Chain { stages } => stages,
            _ => &vec![],
        };

        for (idx, stage_ir) in stages.iter().enumerate() {
            if let Some(stage_impl) = self.stages.get(idx) {
                let sub_graph = stage_impl.lower(stage_ir, ctx)?;
                for node in sub_graph.nodes {
                    combined_graph.add_node(node);
                }
                for edge in sub_graph.edges {
                    combined_graph.edges.push(edge);
                }
            }
        }

        Ok(combined_graph)
    }

    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        let mut all_nodes = Vec::new();
        let mut all_edges = Vec::new();
        let mut prev_exit: Option<Uuid> = None;
        let mut entry_id: Option<Uuid> = None;

        for stage in &self.stages {
            let sub = stage.apply(node);
            if let Some(prev) = prev_exit {
                all_edges.push(ExecutionEdge {
                    from: prev,
                    to: sub.entry_node_id,
                    condition: None,
                });
            } else {
                entry_id = Some(sub.entry_node_id);
            }
            prev_exit = Some(sub.exit_node_id);
            all_nodes.extend(sub.nodes);
            all_edges.extend(sub.edges);
        }

        ExecutionSubgraph {
            nodes: all_nodes,
            edges: all_edges,
            entry_node_id: entry_id.unwrap_or(node.id),
            exit_node_id: prev_exit.unwrap_or(node.id),
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

    struct MockStrategy;

    impl Strategy for MockStrategy {
        fn descriptor(&self) -> StrategyDescriptor {
            StrategyDescriptor {
                name: "Mock",
                parallelism: Parallelism::Sequential,
                requires_barrier: false,
                supports_streaming: StreamingMode::None,
                retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                expected_outputs: vec![ArtifactKind::Generic],
            }
        }

        fn lower(&self, _ir: &crate::compiler::ir::StrategyIR, _ctx: &CompilationContext) -> Result<crate::compiler::ir::PrimitiveGraph, CompilerDiagnostic> {
            Ok(crate::compiler::ir::PrimitiveGraph::new("mock"))
        }

        fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph {
            let id = Uuid::new_v4();
            ExecutionSubgraph {
                nodes: vec![ExecutionNode {
                    id,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: node.model.clone(),
                    retry_policy: node.retry_policy.clone(),
                    fallback: node.fallback.clone(),
                    config: node.config.clone(),
                }],
                edges: vec![],
                entry_node_id: id,
                exit_node_id: id,
            }
        }
    }

    #[test]
    fn test_chain_two_stages() {
        let strategy = ChainStrategy {
            stages: vec![Box::new(MockStrategy), Box::new(MockStrategy)],
        };
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.nodes.len(), 2);
    }

    #[test]
    fn test_chain_edge_between_stages() {
        let strategy = ChainStrategy {
            stages: vec![Box::new(MockStrategy), Box::new(MockStrategy)],
        };
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.edges.len(), 1);
        assert_eq!(subgraph.edges[0].from, subgraph.nodes[0].id);
        assert_eq!(subgraph.edges[0].to, subgraph.nodes[1].id);
    }

    #[test]
    fn test_chain_three_stages() {
        let strategy = ChainStrategy {
            stages: vec![Box::new(MockStrategy), Box::new(MockStrategy), Box::new(MockStrategy)],
        };
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.nodes.len(), 3);
        assert_eq!(subgraph.edges.len(), 2);
    }
}
