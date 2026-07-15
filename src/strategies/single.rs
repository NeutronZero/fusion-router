use uuid::Uuid;

use super::Strategy;
use crate::types::{ExecutionNode, ExecutionNodeKind, ExecutionSubgraph, StrategyKind};

pub struct SingleStrategy;

impl Strategy for SingleStrategy {
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
