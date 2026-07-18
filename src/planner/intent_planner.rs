use std::collections::HashMap;

use async_trait::async_trait;
use uuid::Uuid;

use super::Planner;
use crate::types::execution::ExecutionIntent;
use crate::types::{
    ComplexityLevel, EvidenceSnapshot, IRMetadata, IRNode, IRNodeKind, Intent,
    Policy, Requirements, StrategyKind, WorkflowIR,
};

pub struct IntentPlanner;

impl IntentPlanner {
    fn build_quality(&self, model: &str) -> WorkflowIR {
        let plan_id = Uuid::new_v4();

        let nodes = vec![
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(format!("{}-a", model)),
                config: HashMap::new(),
            },
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(format!("{}-b", model)),
                config: HashMap::new(),
            },
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(format!("{}-c", model)),
                config: HashMap::new(),
            },
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Judge,
                strategy: StrategyKind::Single,
                model: Some(format!("{}-judge", model)),
                config: HashMap::new(),
            },
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Reflection,
                model: Some(model.to_string()),
                config: HashMap::new(),
            },
        ];

        let edges = vec![];

        WorkflowIR {
            plan_id,
            nodes,
            edges,
            metadata: IRMetadata {
                policy_applied: vec!["intent:quality".into()],
                estimated_cost: 0.05,
                estimated_tokens: 5000,
            },
        }
    }

    fn build_speed(&self, model: &str) -> WorkflowIR {
        let plan_id = Uuid::new_v4();

        let nodes = vec![IRNode {
            id: Uuid::new_v4(),
            kind: IRNodeKind::Generate,
            strategy: StrategyKind::Single,
            model: Some(model.to_string()),
            config: HashMap::new(),
        }];

        WorkflowIR {
            plan_id,
            nodes,
            edges: vec![],
            metadata: IRMetadata {
                policy_applied: vec!["intent:speed".into()],
                estimated_cost: 0.01,
                estimated_tokens: 1000,
            },
        }
    }

    fn build_balanced(&self, model: &str) -> WorkflowIR {
        let plan_id = Uuid::new_v4();

        let nodes = vec![
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(format!("{}-a", model)),
                config: HashMap::new(),
            },
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(format!("{}-b", model)),
                config: HashMap::new(),
            },
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Judge,
                strategy: StrategyKind::Single,
                model: Some(format!("{}-judge", model)),
                config: HashMap::new(),
            },
        ];

        WorkflowIR {
            plan_id,
            nodes,
            edges: vec![],
            metadata: IRMetadata {
                policy_applied: vec!["intent:balanced".into()],
                estimated_cost: 0.03,
                estimated_tokens: 3000,
            },
        }
    }

    fn build_exhaustive(&self, model: &str) -> WorkflowIR {
        let plan_id = Uuid::new_v4();

        let nodes = vec![
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(format!("{}-a", model)),
                config: HashMap::new(),
            },
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(format!("{}-b", model)),
                config: HashMap::new(),
            },
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(format!("{}-c", model)),
                config: HashMap::new(),
            },
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Judge,
                strategy: StrategyKind::Single,
                model: Some(format!("{}-judge", model)),
                config: HashMap::new(),
            },
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Reflection,
                model: Some(format!("{}-reflect", model)),
                config: HashMap::new(),
            },
            IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Judge,
                strategy: StrategyKind::Consensus,
                model: Some(format!("{}-final-judge", model)),
                config: HashMap::new(),
            },
        ];

        WorkflowIR {
            plan_id,
            nodes,
            edges: vec![],
            metadata: IRMetadata {
                policy_applied: vec!["intent:exhaustive".into()],
                estimated_cost: 0.08,
                estimated_tokens: 8000,
            },
        }
    }

    fn select_model(requirements: &Requirements) -> String {
        match requirements.intent_classification {
            Intent::Code | Intent::Debug | Intent::Architecture => "claude-sonnet-4-20250514",
            Intent::Analysis => "claude-sonnet-4-20250514",
            Intent::Creative => "claude-sonnet-4-20250514",
            Intent::General => "claude-sonnet-4-20250514",
        }
        .to_string()
    }
}

#[async_trait]
impl Planner for IntentPlanner {
    async fn plan(
        &self,
        requirements: &Requirements,
        _policies: &[Policy],
        _evidence: Option<&EvidenceSnapshot>,
    ) -> WorkflowIR {
        let model = Self::select_model(requirements);

        match &requirements.execution_intent {
            Some(ExecutionIntent::Quality) => self.build_quality(&model),
            Some(ExecutionIntent::Speed) => self.build_speed(&model),
            Some(ExecutionIntent::Balanced) => self.build_balanced(&model),
            Some(ExecutionIntent::Exhaustive) => self.build_exhaustive(&model),
            Some(ExecutionIntent::Constrained { max_cost_usd, .. }) => {
                if let Some(cost) = max_cost_usd {
                    if *cost < 0.02 {
                        return self.build_speed(&model);
                    }
                }
                self.build_balanced(&model)
            }
            None => {
                match requirements.complexity {
                    ComplexityLevel::Critical => self.build_quality(&model),
                    ComplexityLevel::High => self.build_balanced(&model),
                    ComplexityLevel::Medium | ComplexityLevel::Low => self.build_speed(&model),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn make_reqs(execution_intent: Option<ExecutionIntent>) -> Requirements {
        Requirements {
            intent_classification: Intent::General,
            complexity: ComplexityLevel::Medium,
            has_files: false,
            context_window: 4096,
            original_text: "test".to_string(),
            execution_intent,
            output_preferences: None,
        }
    }

    #[tokio::test]
    async fn test_quality_plan_node_count() {
        let planner = IntentPlanner;
        let reqs = make_reqs(Some(ExecutionIntent::Quality));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 5);
        assert_eq!(ir.metadata.estimated_cost, 0.05);
        assert_eq!(ir.metadata.estimated_tokens, 5000);
        assert!(ir.metadata.policy_applied.contains(&"intent:quality".to_string()));
    }

    #[tokio::test]
    async fn test_quality_plan_node_kinds() {
        let planner = IntentPlanner;
        let reqs = make_reqs(Some(ExecutionIntent::Quality));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 5);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[3].kind, IRNodeKind::Judge);
        assert_eq!(ir.nodes[4].strategy, StrategyKind::Reflection);
    }

    #[tokio::test]
    async fn test_speed_plan_single_node() {
        let planner = IntentPlanner;
        let reqs = make_reqs(Some(ExecutionIntent::Speed));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 1);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[0].strategy, StrategyKind::Single);
        assert!(ir.metadata.policy_applied.contains(&"intent:speed".to_string()));
        assert_eq!(ir.metadata.estimated_cost, 0.01);
        assert_eq!(ir.metadata.estimated_tokens, 1000);
    }

    #[tokio::test]
    async fn test_balanced_plan_three_nodes() {
        let planner = IntentPlanner;
        let reqs = make_reqs(Some(ExecutionIntent::Balanced));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 3);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[1].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[2].kind, IRNodeKind::Judge);
        assert!(ir.metadata.policy_applied.contains(&"intent:balanced".to_string()));
        assert_eq!(ir.metadata.estimated_cost, 0.03);
    }

    #[tokio::test]
    async fn test_exhaustive_plan_six_nodes() {
        let planner = IntentPlanner;
        let reqs = make_reqs(Some(ExecutionIntent::Exhaustive));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 6);
        assert!(ir.metadata.policy_applied.contains(&"intent:exhaustive".to_string()));
        assert_eq!(ir.metadata.estimated_cost, 0.08);
        assert_eq!(ir.metadata.estimated_tokens, 8000);
    }

    #[tokio::test]
    async fn test_exhaustive_plan_ends_with_consensus_judge() {
        let planner = IntentPlanner;
        let reqs = make_reqs(Some(ExecutionIntent::Exhaustive));
        let ir = planner.plan(&reqs, &[], None).await;
        let last = ir.nodes.last().unwrap();
        assert_eq!(last.kind, IRNodeKind::Judge);
        assert_eq!(last.strategy, StrategyKind::Consensus);
    }

    #[tokio::test]
    async fn test_constrained_cheap_returns_speed() {
        let planner = IntentPlanner;
        let reqs = make_reqs(Some(ExecutionIntent::Constrained {
            max_latency_ms: None,
            max_cost_usd: Some(0.01),
            max_tokens: None,
            min_confidence: None,
        }));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 1);
    }

    #[tokio::test]
    async fn test_constrained_generous_budget_returns_balanced() {
        let planner = IntentPlanner;
        let reqs = make_reqs(Some(ExecutionIntent::Constrained {
            max_latency_ms: None,
            max_cost_usd: Some(0.05),
            max_tokens: None,
            min_confidence: None,
        }));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 3);
    }

    #[tokio::test]
    async fn test_constrained_no_budget_returns_balanced() {
        let planner = IntentPlanner;
        let reqs = make_reqs(Some(ExecutionIntent::Constrained {
            max_latency_ms: None,
            max_cost_usd: None,
            max_tokens: None,
            min_confidence: None,
        }));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 3);
    }

    #[tokio::test]
    async fn test_constrained_exact_at_threshold_returns_balanced() {
        let planner = IntentPlanner;
        let reqs = make_reqs(Some(ExecutionIntent::Constrained {
            max_latency_ms: None,
            max_cost_usd: Some(0.02),
            max_tokens: None,
            min_confidence: None,
        }));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 3);
    }

    #[tokio::test]
    async fn test_no_intent_critical_complexity_returns_quality() {
        let planner = IntentPlanner;
        let mut reqs = make_reqs(None);
        reqs.complexity = ComplexityLevel::Critical;
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 5);
    }

    #[tokio::test]
    async fn test_no_intent_high_complexity_returns_balanced() {
        let planner = IntentPlanner;
        let mut reqs = make_reqs(None);
        reqs.complexity = ComplexityLevel::High;
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 3);
    }

    #[tokio::test]
    async fn test_no_intent_medium_complexity_returns_speed() {
        let planner = IntentPlanner;
        let mut reqs = make_reqs(None);
        reqs.complexity = ComplexityLevel::Medium;
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 1);
    }

    #[tokio::test]
    async fn test_no_intent_low_complexity_returns_speed() {
        let planner = IntentPlanner;
        let mut reqs = make_reqs(None);
        reqs.complexity = ComplexityLevel::Low;
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 1);
    }

    #[tokio::test]
    async fn test_select_model_returns_non_empty_string() {
        let model = IntentPlanner::select_model(&make_reqs(None));
        assert!(!model.is_empty());
    }

    #[tokio::test]
    async fn test_each_intent_produces_distinct_plan_ids() {
        let planner = IntentPlanner;
        let reqs = make_reqs(Some(ExecutionIntent::Quality));
        let ir1 = planner.plan(&reqs, &[], None).await;
        let reqs = make_reqs(Some(ExecutionIntent::Speed));
        let ir2 = planner.plan(&reqs, &[], None).await;
        assert_ne!(ir1.plan_id, ir2.plan_id);
    }

    #[tokio::test]
    async fn test_plan_nodes_have_unique_ids() {
        let planner = IntentPlanner;
        let reqs = make_reqs(Some(ExecutionIntent::Exhaustive));
        let ir = planner.plan(&reqs, &[], None).await;
        let mut ids: Vec<_> = ir.nodes.iter().map(|n| n.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ir.nodes.len());
    }

    #[tokio::test]
    async fn test_all_plans_have_empty_edges() {
        let planner = IntentPlanner;
        for intent in &[ExecutionIntent::Quality, ExecutionIntent::Speed, ExecutionIntent::Balanced, ExecutionIntent::Exhaustive] {
            let reqs = match intent {
                ExecutionIntent::Constrained { .. } => unreachable!(),
                other => make_reqs(Some(other.clone())),
            };
            let ir = planner.plan(&reqs, &[], None).await;
            assert!(ir.edges.is_empty(), "Plan for {:?} should have no edges", intent);
        }
    }
}
