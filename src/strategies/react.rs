use std::sync::Arc;

use super::{Parallelism, Strategy, StrategyDescriptor, StreamingMode};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{PrimitiveGraph, PrimitiveNode, PrimitiveNodeKind, StrategyIR};
use crate::tools::ToolRegistry;
use crate::types::{ArtifactKind, RetryPolicy};

pub struct ReActStrategy {
    pub max_iterations: u32,
    pub tool_registry: Option<Arc<ToolRegistry>>,
}

impl Default for ReActStrategy {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            tool_registry: None,
        }
    }
}

impl ReActStrategy {
    pub fn new(max_iterations: u32, tool_registry: Option<Arc<ToolRegistry>>) -> Self {
        Self {
            max_iterations,
            tool_registry,
        }
    }
}

impl Strategy for ReActStrategy {
    fn descriptor(&self) -> StrategyDescriptor {
        StrategyDescriptor {
            name: "ReAct".into(),
            parallelism: Parallelism::Sequential,
            requires_barrier: false,
            supports_streaming: StreamingMode::None,
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
        _ctx: &CompilationContext,
    ) -> Result<PrimitiveGraph, CompilerDiagnostic> {
        let max_iterations = match ir {
            StrategyIR::ReAct { max_iterations } => *max_iterations,
            _ => self.max_iterations,
        };

        let mut graph = PrimitiveGraph::new("react_graph");
        graph.add_node(PrimitiveNode {
            id: "react_loop".into(),
            kind: PrimitiveNodeKind::FeedbackLoop { max_iterations },
            artifact_kind: Some("Generic".into()),
        });

        Ok(graph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::StrategyIR;

    #[test]
    fn test_react_config_has_max_iterations() {
        let strategy = ReActStrategy::default();
        let ctx = CompilationContext::new();
        let graph = strategy
            .lower(&StrategyIR::ReAct { max_iterations: 10 }, &ctx)
            .unwrap();
        let loop_node = &graph.nodes[0];
        assert!(matches!(
            loop_node.kind,
            PrimitiveNodeKind::FeedbackLoop { max_iterations: 10 }
        ));
    }
}
