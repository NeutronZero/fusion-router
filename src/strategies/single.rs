use uuid::Uuid;

use super::{Parallelism, StreamingMode, Strategy, StrategyDescriptor};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{PrimitiveGraph, PrimitiveNode, PrimitiveNodeKind, StrategyIR};
use crate::types::{ArtifactKind, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph, RetryPolicy, StrategyKind};

pub struct SingleStrategy;

impl Strategy for SingleStrategy {
    fn descriptor(&self) -> StrategyDescriptor {
        StrategyDescriptor {
            name: "Single",
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

    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        let gen_node = ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: node.model.clone(),
            retry_policy: node.retry_policy.clone(),
            fallback: node.fallback.clone(),
            config: node.config.clone(),
        };

        let entry_id = gen_node.id;

        ExecutionSubgraph {
            nodes: vec![gen_node],
            edges: vec![],
            entry_node_id: entry_id,
            exit_node_id: entry_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ExecutionNode, ExecutionNodeKind, StrategyKind};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_test_node() -> ExecutionNode {
        ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: "gpt-4".to_string(),
            retry_policy: crate::types::RetryPolicy { max_retries: 3, backoff_ms: 1000 },
            fallback: None,
            config: HashMap::new(),
        }
    }

    #[test]
    fn test_single_strategy_produces_one_node() {
        let strategy = SingleStrategy;
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.nodes.len(), 1);
    }

    #[test]
    fn test_single_strategy_node_kind() {
        let strategy = SingleStrategy;
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert!(matches!(subgraph.nodes[0].kind, ExecutionNodeKind::LLMGenerate));
    }

    #[test]
    fn test_single_strategy_uses_node_model_and_config() {
        let strategy = SingleStrategy;
        let mut node = make_test_node();
        node.model = "claude-3".to_string();
        node.config.insert("temperature".into(), serde_json::json!(0.7));
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.nodes[0].model, "claude-3");
        assert_eq!(
            subgraph.nodes[0].config.get("temperature").and_then(|v| v.as_f64()),
            Some(0.7)
        );
    }

    #[test]
    fn test_single_strategy_lowering() {
        let strategy = SingleStrategy;
        let ctx = CompilationContext::new();
        let graph = strategy.lower(&StrategyIR::Single, &ctx).unwrap();
        assert_eq!(graph.nodes.len(), 1);
        assert!(matches!(graph.nodes[0].kind, PrimitiveNodeKind::LLMGenerate { .. }));
    }
}
