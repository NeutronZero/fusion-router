use uuid::Uuid;

use super::Strategy;
use crate::types::{
    ExecutionEdge, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph, RetryPolicy, StrategyKind,
};

pub struct ReflectionStrategy;

impl Strategy for ReflectionStrategy {
    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        let gen_id = Uuid::new_v4();
        let review_id = Uuid::new_v4();
        let gate_id = Uuid::new_v4();

        let gen_node = ExecutionNode {
            id: gen_id,
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Reflection,
            model: node.model.clone(),
            retry_policy: node.retry_policy.clone(),
            fallback: node.fallback.clone(),
            config: node.config.clone(),
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
            config: Default::default(),
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
            config: Default::default(),
        };

        ExecutionSubgraph {
            nodes: vec![gen_node, review_node, gate_node],
            edges: vec![
                ExecutionEdge { from: gen_id, to: review_id },
                ExecutionEdge { from: review_id, to: gate_id },
            ],
            entry_node_id: gen_id,
            exit_node_id: gate_id,
        }
    }
}
