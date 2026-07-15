use std::collections::HashMap;
use async_trait::async_trait;
use uuid::Uuid;

use super::Planner;
use crate::types::{
    Complexity, EvidenceSnapshot, IRMetadata, IRNode, IRNodeKind, Intent,
    Policy, Requirements, StrategyKind, WorkflowIR,
};

pub struct SimplePlanner;

#[async_trait]
impl Planner for SimplePlanner {
    async fn plan(
        &self,
        requirements: &Requirements,
        _policies: &[Policy],
        _evidence: Option<&EvidenceSnapshot>,
    ) -> WorkflowIR {
        let plan_id = Uuid::new_v4();

        let strategy = select_strategy(requirements);
        let model = select_model(requirements);

        let generate_node = IRNode {
            id: Uuid::new_v4(),
            kind: IRNodeKind::Generate,
            strategy,
            model: Some(model),
            config: HashMap::new(),
        };

        let nodes = vec![generate_node];
        let edges = vec![];

        let metadata = IRMetadata {
            policy_applied: vec!["default".to_string()],
            estimated_cost: estimate_cost(requirements),
            estimated_tokens: estimate_tokens(requirements),
        };

        WorkflowIR {
            plan_id,
            nodes,
            edges,
            metadata,
        }
    }
}

fn select_strategy(requirements: &Requirements) -> StrategyKind {
    match requirements.complexity {
        Complexity::Critical => StrategyKind::Consensus,
        Complexity::High => StrategyKind::Reflection,
        Complexity::Medium => StrategyKind::Single,
        Complexity::Low => StrategyKind::Single,
    }
}

fn select_model(requirements: &Requirements) -> String {
    match requirements.intent {
        Intent::Code | Intent::Debug | Intent::Architecture => "claude-sonnet-4-20250514".to_string(),
        Intent::Analysis => "claude-sonnet-4-20250514".to_string(),
        Intent::Creative => "claude-sonnet-4-20250514".to_string(),
        Intent::General => "claude-sonnet-4-20250514".to_string(),
    }
}

fn estimate_cost(_requirements: &Requirements) -> f64 {
    0.01
}

fn estimate_tokens(_requirements: &Requirements) -> u64 {
    1000
}
