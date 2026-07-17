use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use super::Strategy;
use crate::tools::ToolRegistry;
use crate::types::{
    ExecutionEdge, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph, RetryPolicy, StrategyKind,
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
            config: {
                let mut m = HashMap::new();
                m.insert("max_iterations".into(), serde_json::json!(self.max_iterations));
                m
            },
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

        ExecutionSubgraph {
            nodes: vec![loop_node, gen_node],
            edges: vec![
                ExecutionEdge { from: loop_id, to: gen_id, condition: None },
                ExecutionEdge { from: gen_id, to: loop_id, condition: Some("loop".into()) },
            ],
            entry_node_id: loop_id,
            exit_node_id: gen_id,
        }
    }
}
