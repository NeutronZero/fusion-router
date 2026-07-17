use super::Strategy;
use crate::types::{ExecutionEdge, ExecutionNode, ExecutionSubgraph};

pub struct DebateStrategy {
    pub debaters: Vec<Box<dyn Strategy>>,
    pub judge: Box<dyn Strategy>,
}

impl Strategy for DebateStrategy {
    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph {
        let mut all_nodes = Vec::new();
        let mut all_edges = Vec::new();
        let mut debater_exits = Vec::new();
        let mut entry_id = None;

        for debater in &self.debaters {
            let sub = debater.apply(node);
            if entry_id.is_none() {
                entry_id = Some(sub.entry_node_id);
            }
            debater_exits.push(sub.exit_node_id);
            all_nodes.extend(sub.nodes);
            all_edges.extend(sub.edges);
        }

        let judge_sub = self.judge.apply(node);
        for exit_id in &debater_exits {
            all_edges.push(ExecutionEdge {
                from: *exit_id,
                to: judge_sub.entry_node_id,
                condition: None,
            });
        }
        all_nodes.extend(judge_sub.nodes);
        all_edges.extend(judge_sub.edges);

        ExecutionSubgraph {
            nodes: all_nodes,
            edges: all_edges,
            entry_node_id: entry_id.unwrap_or(node.id),
            exit_node_id: judge_sub.exit_node_id,
        }
    }
}
