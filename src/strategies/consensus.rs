use uuid::Uuid;

use super::Strategy;
use crate::types::{
    ExecutionEdge, ExecutionNode, ExecutionNodeKind, ExecutionSubgraph, RetryPolicy, StrategyKind,
};

const DEFAULT_CONSENSUS_COUNT: u32 = 3;

pub struct ConsensusStrategy {
    pub count: u32,
}

impl Default for ConsensusStrategy {
    fn default() -> Self {
        Self { count: DEFAULT_CONSENSUS_COUNT }
    }
}

impl Strategy for ConsensusStrategy {
    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        let mut gen_ids = Vec::new();

        for _ in 0..self.count {
            let gen_id = Uuid::new_v4();
            nodes.push(ExecutionNode {
                id: gen_id,
                kind: ExecutionNodeKind::LLMGenerate,
                strategy: StrategyKind::Single,
                model: node.model.clone(),
                retry_policy: node.retry_policy.clone(),
                fallback: node.fallback.clone(),
                config: node.config.clone(),
            });
            gen_ids.push(gen_id);
        }

        let judge_id = Uuid::new_v4();
        nodes.push(ExecutionNode {
            id: judge_id,
            kind: ExecutionNodeKind::LLMJudge,
            strategy: StrategyKind::Consensus,
            model: node.model.clone(),
            retry_policy: RetryPolicy {
                max_retries: 1,
                backoff_ms: 500,
            },
            fallback: node.fallback.clone(),
            config: Default::default(),
        });

        for gen_id in &gen_ids {
            edges.push(ExecutionEdge {
                from: *gen_id,
                to: judge_id,
                condition: None,
            });
        }

        let entry_node_id = gen_ids[0];

        ExecutionSubgraph {
            nodes,
            edges,
            entry_node_id,
            exit_node_id: judge_id,
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
    fn test_consensus_default_count() {
        let strategy = ConsensusStrategy::default();
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.nodes.len(), 4);
    }

    #[test]
    fn test_consensus_custom_count() {
        let strategy = ConsensusStrategy { count: 5 };
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.nodes.len(), 6);
    }

    #[test]
    fn test_consensus_edges_from_all_generators_to_judge() {
        let strategy = ConsensusStrategy { count: 3 };
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        let judge_id = subgraph.nodes.last().unwrap().id;
        for edge in &subgraph.edges {
            assert_eq!(edge.to, judge_id);
        }
        assert_eq!(subgraph.edges.len(), 3);
    }

    #[test]
    fn test_consensus_judge_kind() {
        let strategy = ConsensusStrategy::default();
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        let judge_node = subgraph.nodes.last().unwrap();
        assert!(matches!(judge_node.kind, ExecutionNodeKind::LLMJudge));
    }

    #[test]
    fn test_consensus_entry_is_first_generator() {
        let strategy = ConsensusStrategy { count: 3 };
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.entry_node_id, subgraph.nodes[0].id);
    }
}
