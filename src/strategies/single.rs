use super::{Parallelism, StreamingMode, Strategy, StrategyDescriptor};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{PrimitiveGraph, PrimitiveNode, PrimitiveNodeKind, StrategyIR};
use crate::types::{ArtifactKind, RetryPolicy};

pub struct SingleStrategy;

impl Strategy for SingleStrategy {
    fn descriptor(&self) -> StrategyDescriptor {
        StrategyDescriptor {
            name: "Single".into(),
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

    fn lower(&self, _ir: &StrategyIR, ctx: &CompilationContext) -> Result<PrimitiveGraph, CompilerDiagnostic> {
        let mut graph = PrimitiveGraph::new("single_graph");
        let model = ctx.available_models.first().cloned().unwrap_or_else(|| "default".into());
        graph.add_node(PrimitiveNode {
            id: "node_1".into(),
            kind: PrimitiveNodeKind::LLMGenerate { model, role: None },
            artifact_kind: Some("Generic".into()),
        });
        Ok(graph)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::context::CompilationContext;
    use crate::compiler::ir::{PrimitiveNodeKind, StrategyIR};

    #[test]
    fn test_single_strategy_lowering() {
        let strategy = SingleStrategy;
        let ctx = CompilationContext::new();
        let graph = strategy.lower(&StrategyIR::Single, &ctx).unwrap();
        assert_eq!(graph.nodes.len(), 1);
        assert!(matches!(graph.nodes[0].kind, PrimitiveNodeKind::LLMGenerate { .. }));
    }
}
