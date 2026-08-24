use std::time::Duration;

use super::{Parallelism, Strategy, StrategyDescriptor, StreamingMode};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{
    BarrierFailurePolicy, PrimitiveGraph, PrimitiveNode, PrimitiveNodeKind, ReducerMode, StrategyIR,
};
use crate::types::{ArtifactKind, RetryPolicy};

pub struct DebateStrategy {
    pub debaters: Vec<Box<dyn Strategy>>,
    pub judge: Box<dyn Strategy>,
}

impl Strategy for DebateStrategy {
    fn descriptor(&self) -> StrategyDescriptor {
        StrategyDescriptor {
            name: "Debate".into(),
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

    fn lower(
        &self,
        ir: &StrategyIR,
        ctx: &CompilationContext,
    ) -> Result<PrimitiveGraph, CompilerDiagnostic> {
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
        let reducer_model = ctx
            .available_models
            .first()
            .cloned()
            .unwrap_or_else(|| "claude-opus-4".into());
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::DebateRole;

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
        assert!(matches!(
            graph.nodes[0].kind,
            PrimitiveNodeKind::FanOut { count: 2 }
        ));
    }
}
