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
    fn test_reflection_produces_three_nodes() {
        let strategy = ReflectionStrategy::default();
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.nodes.len(), 3);
    }

    #[test]
    fn test_reflection_node_kinds() {
        let strategy = ReflectionStrategy::default();
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert!(matches!(subgraph.nodes[0].kind, ExecutionNodeKind::LLMGenerate));
        assert!(matches!(subgraph.nodes[1].kind, ExecutionNodeKind::LLMReview));
        assert!(matches!(subgraph.nodes[2].kind, ExecutionNodeKind::Gate));
    }

    #[test]
    fn test_reflection_edges() {
        let strategy = ReflectionStrategy::default();
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.edges.len(), 2);
        assert_eq!(subgraph.edges[0].from, subgraph.nodes[0].id);
        assert_eq!(subgraph.edges[0].to, subgraph.nodes[1].id);
        assert_eq!(subgraph.edges[1].from, subgraph.nodes[1].id);
        assert_eq!(subgraph.edges[1].to, subgraph.nodes[2].id);
    }

    #[test]
    fn test_reflection_entry_exit() {
        let strategy = ReflectionStrategy::default();
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.entry_node_id, subgraph.nodes[0].id);
        assert_eq!(subgraph.exit_node_id, subgraph.nodes[2].id);
    }

    #[test]
    fn test_reflection_config_carries_timeout() {
        let strategy = ReflectionStrategy { max_reflection_cycles: 3, per_leg_timeout_ms: 5000 };
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        let gen_config = &subgraph.nodes[0].config;
        assert_eq!(
            gen_config.get("per_leg_timeout_ms").and_then(|v| v.as_u64()),
            Some(5000)
        );
    }
}
