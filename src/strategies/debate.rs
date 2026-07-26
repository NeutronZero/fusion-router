use std::time::Duration;

use super::{Parallelism, StreamingMode, Strategy, StrategyDescriptor};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{
    BarrierFailurePolicy, PrimitiveGraph, PrimitiveNode, PrimitiveNodeKind, ReducerMode, StrategyIR,
};
use crate::types::{
    ArtifactKind, ExecutionEdge, ExecutionNode, ExecutionSubgraph, RetryPolicy,
};

pub struct DebateStrategy {
    pub debaters: Vec<Box<dyn Strategy>>,
    pub judge: Box<dyn Strategy>,
}

impl Strategy for DebateStrategy {
    fn descriptor(&self) -> StrategyDescriptor {
        StrategyDescriptor {
            name: "Debate",
            parallelism: Parallelism::Unlimited,
            requires_barrier: true,
            supports_streaming: StreamingMode::IncrementalReduction,
            retry_policy: RetryPolicy {
                max_retries: 2,
                backoff_ms: 1000,
            },
            expected_outputs: vec![ArtifactKind::Debate],
        }
    }

    fn lower(&self, ir: &StrategyIR, ctx: &CompilationContext) -> Result<PrimitiveGraph, CompilerDiagnostic> {
        let roles = match ir {
            StrategyIR::Debate { roles } => roles.clone(),
            _ => Vec::new(),
        };

        let count = roles.len() as u32;
        if count < 2 {
            return Err(CompilerDiagnostic::error(
                "E0102",
                "Debate strategy requires at least 2 roles (e.g. Defender and Critic)",
            ));
        }

        let mut graph = PrimitiveGraph::new("debate_graph");

        // FanOut node
        graph.add_node(PrimitiveNode {
            id: "fanout_debate".into(),
            kind: PrimitiveNodeKind::FanOut { count },
            artifact_kind: None,
        });

        // Debater nodes
        for (idx, role) in roles.iter().enumerate() {
            let debater_id = format!("debater_{}", idx + 1);
            graph.add_node(PrimitiveNode {
                id: debater_id.clone(),
                kind: PrimitiveNodeKind::LLMGenerate {
                    model: role.model.clone(),
                    role: Some(role.name.clone()),
                },
                artifact_kind: Some("Debate".into()),
            });

            graph.add_edge("fanout_debate", debater_id.clone(), None);
            graph.add_edge(debater_id, "barrier_debate", None);
        }

        // Barrier node
        graph.add_node(PrimitiveNode {
            id: "barrier_debate".into(),
            kind: PrimitiveNodeKind::Barrier {
                min_completion: 1.0,
                timeout: Duration::from_secs(60),
                on_failure: BarrierFailurePolicy::Continue,
            },
            artifact_kind: None,
        });

        // Reducer node
        let reducer_model = ctx.available_models.first().cloned().unwrap_or_else(|| "claude-opus-4".into());
        graph.add_node(PrimitiveNode {
            id: "reducer_debate".into(),
            kind: PrimitiveNodeKind::Reducer {
                mode: ReducerMode::Debate,
                model: reducer_model,
            },
            artifact_kind: Some("Debate".into()),
        });

        graph.add_edge("barrier_debate", "reducer_debate", None);

        Ok(graph)
    }

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
    use crate::compiler::ir::DebateRole;

    #[test]
    fn test_debate_produces_two_debaters_and_judge() {
        let strategy = DebateStrategy {
            debaters: vec![
                Box::new(crate::strategies::single::SingleStrategy),
                Box::new(crate::strategies::single::SingleStrategy),
            ],
            judge: Box::new(crate::strategies::single::SingleStrategy),
        };
        let node = crate::types::ExecutionNode {
            id: uuid::Uuid::new_v4(),
            kind: crate::types::ExecutionNodeKind::LLMGenerate,
            strategy: crate::types::StrategyKind::Debate,
            model: "gpt-4".to_string(),
            retry_policy: crate::types::RetryPolicy { max_retries: 3, backoff_ms: 1000 },
            fallback: None,
            config: std::collections::HashMap::new(),
        };
        let subgraph = strategy.apply(&node);
        assert!(subgraph.nodes.len() >= 3);
        let edges_to_judge = subgraph.edges.iter().filter(|e| e.to == subgraph.exit_node_id).count();
        assert!(edges_to_judge >= 2);
    }

    #[test]
    fn test_debate_lowering() {
        let strategy = DebateStrategy {
            debaters: vec![],
            judge: Box::new(crate::strategies::single::SingleStrategy),
        };
        let ctx = CompilationContext::new();
        let ir = StrategyIR::Debate {
            roles: vec![
                DebateRole {
                    name: "Defender".into(),
                    model: "claude-opus-4".into(),
                    stance: "Defend".into(),
                },
                DebateRole {
                    name: "Critic".into(),
                    model: "gpt-4o".into(),
                    stance: "Critique".into(),
                },
            ],
        };

        let graph = strategy.lower(&ir, &ctx).unwrap();
        // 1 FanOut + 2 Debaters + 1 Barrier + 1 Reducer = 5 nodes
        assert_eq!(graph.nodes.len(), 5);
        assert!(matches!(graph.nodes[0].kind, PrimitiveNodeKind::FanOut { count: 2 }));
    }
}
