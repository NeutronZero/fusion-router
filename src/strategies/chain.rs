use uuid::Uuid;

use super::Strategy;
use crate::types::{ExecutionEdge, ExecutionNode, ExecutionSubgraph};

pub struct ChainStrategy {
    pub stages: Vec<Box<dyn Strategy>>,
}

impl Strategy for ChainStrategy {
    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        let mut all_nodes = Vec::new();
        let mut all_edges = Vec::new();
        let mut prev_exit: Option<Uuid> = None;
        let mut entry_id: Option<Uuid> = None;

        for stage in &self.stages {
            let sub = stage.apply(node);
            if let Some(prev) = prev_exit {
                all_edges.push(ExecutionEdge {
                    from: prev,
                    to: sub.entry_node_id,
                    condition: None,
                });
            } else {
                entry_id = Some(sub.entry_node_id);
            }
            prev_exit = Some(sub.exit_node_id);
            all_nodes.extend(sub.nodes);
            all_edges.extend(sub.edges);
        }

        ExecutionSubgraph {
            nodes: all_nodes,
            edges: all_edges,
            entry_node_id: entry_id.unwrap_or(node.id),
            exit_node_id: prev_exit.unwrap_or(node.id),
        }
    }
}
