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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ExecutionNode, ExecutionNodeKind, ExecutionSubgraph, StrategyKind};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_test_node() -> ExecutionNode {
        ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: "gpt-4".to_string(),
            retry_policy: crate::types::RetryPolicy { max_retries: 3, backoff_ms: 1000 },
            fallback: None,
            config: HashMap::new(),
        }
    }

    struct MockStrategy;

    impl Strategy for MockStrategy {
        fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph {
            let id = Uuid::new_v4();
            ExecutionSubgraph {
                nodes: vec![ExecutionNode {
                    id,
                    kind: ExecutionNodeKind::LLMGenerate,
                    strategy: StrategyKind::Single,
                    model: node.model.clone(),
                    retry_policy: node.retry_policy.clone(),
                    fallback: node.fallback.clone(),
                    config: node.config.clone(),
                }],
                edges: vec![],
                entry_node_id: id,
                exit_node_id: id,
            }
        }
    }

    #[test]
    fn test_debate_two_debaters() {
        let strategy = DebateStrategy {
            debaters: vec![Box::new(MockStrategy), Box::new(MockStrategy)],
            judge: Box::new(MockStrategy),
        };
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.nodes.len(), 3);
    }

    #[test]
    fn test_debate_edges_to_judge() {
        let strategy = DebateStrategy {
            debaters: vec![Box::new(MockStrategy), Box::new(MockStrategy)],
            judge: Box::new(MockStrategy),
        };
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        let judge_entry_id = subgraph.nodes[2].id;
        assert_eq!(subgraph.edges.len(), 2);
        assert_eq!(subgraph.edges[0].to, judge_entry_id);
        assert_eq!(subgraph.edges[1].to, judge_entry_id);
    }

    #[test]
    fn test_debate_entry_is_first_debater_entry() {
        let strategy = DebateStrategy {
            debaters: vec![Box::new(MockStrategy), Box::new(MockStrategy)],
            judge: Box::new(MockStrategy),
        };
        let node = make_test_node();
        let subgraph = strategy.apply(&node);
        assert_eq!(subgraph.entry_node_id, subgraph.nodes[0].id);
    }
}
