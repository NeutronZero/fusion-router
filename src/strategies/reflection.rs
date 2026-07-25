use uuid::Uuid;

use super::Strategy;
use crate::types::{
    ExecutionEdge, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph, RetryPolicy, StrategyKind,
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
    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        let gen_id = Uuid::new_v4();
        let review_id = Uuid::new_v4();
        let gate_id = Uuid::new_v4();

        let mut gen_config = node.config.clone();
        gen_config.insert("per_leg_timeout_ms".into(), serde_json::json!(self.per_leg_timeout_ms));

        let gen_node = ExecutionNode {
            id: gen_id,
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Reflection,
            model: node.model.clone(),
            retry_policy: node.retry_policy.clone(),
            fallback: node.fallback.clone(),
            config: gen_config,
        };

        let review_node = ExecutionNode {
            id: review_id,
            kind: ExecutionNodeKind::LLMReview,
            strategy: StrategyKind::Reflection,
            model: node.model.clone(),
            retry_policy: RetryPolicy {
                max_retries: 1,
                backoff_ms: 500,
            },
            fallback: None,
            config: {
                let mut m = std::collections::HashMap::new();
                m.insert("per_leg_timeout_ms".into(), serde_json::json!(self.per_leg_timeout_ms));
                m
            },
        };

        let gate_node = ExecutionNode {
            id: gate_id,
            kind: ExecutionNodeKind::Gate,
            strategy: StrategyKind::Reflection,
            model: node.model.clone(),
            retry_policy: RetryPolicy {
                max_retries: 1,
                backoff_ms: 500,
            },
            fallback: None,
            config: {
                let mut m = std::collections::HashMap::new();
                m.insert("max_reflection_cycles".into(), serde_json::json!(self.max_reflection_cycles));
                m
            },
        };

        ExecutionSubgraph {
            nodes: vec![gen_node, review_node, gate_node],
            edges: vec![
                ExecutionEdge { from: gen_id, to: review_id, condition: None },
                ExecutionEdge { from: review_id, to: gate_id, condition: None },
            ],
            entry_node_id: gen_id,
            exit_node_id: gate_id,
        }
    }
}
