use super::{Parallelism, StreamingMode, Strategy, StrategyDescriptor};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{PrimitiveGraph, PrimitiveNode, PrimitiveNodeKind, StrategyIR};
use crate::types::{
    ArtifactKind, RetryPolicy,
};

pub struct ReflectionStrategy {
    pub max_reflection_cycles: u32,
    pub per_leg_timeout_ms: u64,
}

impl Default for ReflectionStrategy {
    fn default() -> Self {
        Self {
            max_reflection_cycles: 3,
            per_leg_timeout_ms: 30000,
        }
    }
}

impl Strategy for ReflectionStrategy {
    fn descriptor(&self) -> StrategyDescriptor {
        StrategyDescriptor {
            name: "Reflection".into(),
            parallelism: Parallelism::Sequential,
            requires_barrier: false,
            supports_streaming: StreamingMode::None,
            retry_policy: RetryPolicy {
                max_retries: 2,
                backoff_ms: 1000,
            },
            expected_outputs: vec![ArtifactKind::Reflection],
        }
    }

    fn lower(&self, ir: &StrategyIR, ctx: &CompilationContext) -> Result<PrimitiveGraph, CompilerDiagnostic> {
        let max_cycles = match ir {
            StrategyIR::Reflection { max_cycles } => *max_cycles,
            _ => self.max_reflection_cycles,
        };

        let mut graph = PrimitiveGraph::new("reflection_graph");
        let model = ctx.available_models.first().cloned().unwrap_or_else(|| "default".into());

        graph.add_node(PrimitiveNode {
            id: "worker_1".into(),
            kind: PrimitiveNodeKind::LLMGenerate {
                model: model.clone(),
                role: Some("Worker".into()),
            },
            artifact_kind: Some("Reflection".into()),
        });

        graph.add_node(PrimitiveNode {
            id: "reviewer_1".into(),
            kind: PrimitiveNodeKind::LLMReview {
                model: model.clone(),
            },
            artifact_kind: Some("Reflection".into()),
        });

        graph.add_node(PrimitiveNode {
            id: "branch_1".into(),
            kind: PrimitiveNodeKind::ConditionalBranch {
                condition: "approved == false".into(),
            },
            artifact_kind: None,
        });

        graph.add_node(PrimitiveNode {
            id: "loop_1".into(),
            kind: PrimitiveNodeKind::FeedbackLoop {
                max_iterations: max_cycles,
            },
            artifact_kind: None,
        });

        graph.add_edge("worker_1", "reviewer_1", None);
        graph.add_edge("reviewer_1", "branch_1", None);
        graph.add_edge("branch_1", "loop_1", Some("critique_present".into()));

        Ok(graph)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::StrategyIR;

    #[test]
    fn test_reflection_lowering() {
        let strategy = ReflectionStrategy::default();
        let ctx = CompilationContext::new();
        let graph = strategy.lower(&StrategyIR::Reflection { max_cycles: 3 }, &ctx).unwrap();
        assert_eq!(graph.nodes.len(), 4);
    }
}
