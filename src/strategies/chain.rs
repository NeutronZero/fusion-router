use super::{Parallelism, Strategy, StrategyDescriptor, StreamingMode};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{PrimitiveGraph, StrategyIR};
use crate::types::{ArtifactKind, RetryPolicy};

pub struct ChainStrategy {
    pub stages: Vec<Box<dyn Strategy>>,
}

impl Strategy for ChainStrategy {
    fn descriptor(&self) -> StrategyDescriptor {
        StrategyDescriptor {
            name: "Chain".into(),
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

    fn lower(
        &self,
        ir: &StrategyIR,
        ctx: &CompilationContext,
    ) -> Result<PrimitiveGraph, CompilerDiagnostic> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::context::CompilationContext;
    use crate::compiler::diagnostics::CompilerDiagnostic;
    use crate::compiler::ir::{PrimitiveNode, PrimitiveNodeKind, StrategyIR};

    struct MockStrategy(pub u32);

    impl Strategy for MockStrategy {
        fn descriptor(&self) -> StrategyDescriptor {
            StrategyDescriptor {
                name: "Mock".into(),
                parallelism: Parallelism::Sequential,
                requires_barrier: false,
                supports_streaming: StreamingMode::None,
                retry_policy: RetryPolicy {
                    max_retries: 0,
                    backoff_ms: 0,
                },
                expected_outputs: vec![ArtifactKind::Generic],
            }
        }

        fn lower(
            &self,
            _ir: &StrategyIR,
            _ctx: &CompilationContext,
        ) -> Result<PrimitiveGraph, CompilerDiagnostic> {
            let mut graph = PrimitiveGraph::new(format!("mock_{}", self.0));
            graph.add_node(PrimitiveNode {
                id: format!("mock_node_{}", self.0),
                kind: PrimitiveNodeKind::LLMGenerate {
                    model: "default".into(),
                    role: None,
                },
                artifact_kind: Some("Generic".into()),
            });
            Ok(graph)
        }
    }

    #[test]
    fn test_chain_two_stages() {
        let strategy = ChainStrategy {
            stages: vec![Box::new(MockStrategy(1)), Box::new(MockStrategy(2))],
        };
        let ctx = CompilationContext::new();
        let ir = StrategyIR::Chain {
            stages: vec![StrategyIR::Single, StrategyIR::Single],
        };
        let pg = strategy.lower(&ir, &ctx).unwrap();
        assert_eq!(pg.nodes.len(), 2);
    }
}
