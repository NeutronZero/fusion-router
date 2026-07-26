use std::sync::Arc;
use uuid::Uuid;

use super::{Parallelism, StreamingMode, Strategy, StrategyDescriptor};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{PrimitiveGraph, PrimitiveNode, PrimitiveNodeKind, StrategyIR};
use crate::tools::ToolRegistry;
use crate::types::{
    ArtifactKind, ExecutionEdge, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph, RetryPolicy, StrategyKind,
};

pub struct ReActStrategy {
    pub max_iterations: u32,
    pub tool_registry: Option<Arc<ToolRegistry>>,
}

impl Default for ReActStrategy {
    fn default() -> Self {
        Self { max_iterations: 10, tool_registry: None }
    }
}

impl ReActStrategy {
    pub fn new(max_iterations: u32, tool_registry: Option<Arc<ToolRegistry>>) -> Self {
        Self { max_iterations, tool_registry }
    }
}

impl Strategy for ReActStrategy {
    fn descriptor(&self) -> StrategyDescriptor {
        StrategyDescriptor {
            name: "ReAct",
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

    fn lower(&self, ir: &StrategyIR, _ctx: &CompilationContext) -> Result<PrimitiveGraph, CompilerDiagnostic> {
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

    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        let loop_id = Uuid::new_v4();
        let gen_id = Uuid::new_v4();

        let mut config = node.config.clone();
        config.insert("max_iterations".into(), serde_json::json!(self.max_iterations));
        if let Some(ref registry) = self.tool_registry {
            let tool_names: Vec<&str> = registry.list();
            config.insert("available_tools".into(), serde_json::json!(tool_names));
        }

        let loop_node = ExecutionNode {
            id: loop_id,
            kind: ExecutionNodeKind::Loop,
            strategy: StrategyKind::Single,
            model: String::new(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config: config.clone(),
        };

        let gen_node = ExecutionNode {
            id: gen_id,
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: node.model.clone(),
            retry_policy: node.retry_policy.clone(),
            fallback: node.fallback.clone(),
            config,
        };

        let edges = vec![
            ExecutionEdge {
                from: loop_id,
                to: gen_id,
                condition: None,
            },
            ExecutionEdge {
                from: gen_id,
                to: loop_id,
                condition: Some("loop".into()),
            },
        ];

        ExecutionSubgraph {
            nodes: vec![loop_node, gen_node],
            edges,
            entry_node_id: loop_id,
            exit_node_id: gen_id,
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
    fn test_react_produces_two_nodes() {
        let strategy = ReActStrategy::default();
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.nodes.len(), 2);
    }

    #[test]
    fn test_react_edges_have_loop_condition() {
        let strategy = ReActStrategy::default();
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.edges.len(), 2);
        assert_eq!(subgraph.edges[1].condition, Some("loop".to_string()));
    }

    #[test]
    fn test_react_entry_is_loop() {
        let strategy = ReActStrategy::default();
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.entry_node_id, subgraph.nodes[0].id);
        assert!(matches!(subgraph.nodes[0].kind, ExecutionNodeKind::Loop));
    }

    #[test]
    fn test_react_config_has_max_iterations() {
        let strategy = ReActStrategy::default();
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        let loop_config = &subgraph.nodes[0].config;
        assert_eq!(
            loop_config.get("max_iterations").and_then(|v| v.as_u64()),
            Some(10)
        );
    }

    #[test]
    fn test_react_custom_max_iterations() {
        let strategy = ReActStrategy { max_iterations: 5, tool_registry: None };
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        let loop_config = &subgraph.nodes[0].config;
        assert_eq!(
            loop_config.get("max_iterations").and_then(|v| v.as_u64()),
            Some(5)
        );
    }
}
