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
