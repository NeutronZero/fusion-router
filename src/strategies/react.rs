use std::collections::HashMap;
use uuid::Uuid;

use super::Strategy;
use crate::types::{
    ExecutionEdge, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph, RetryPolicy, StrategyKind,
};

pub struct ReActStrategy {
    pub max_iterations: u32,
}

impl Default for ReActStrategy {
    fn default() -> Self {
        Self { max_iterations: 10 }
    }
}

impl Strategy for ReActStrategy {
    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        let loop_id = Uuid::new_v4();
        let gen_id = Uuid::new_v4();

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
            config: node.config.clone(),
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
