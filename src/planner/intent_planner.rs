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
