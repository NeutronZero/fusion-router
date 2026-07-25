use std::collections::HashMap;
use async_trait::async_trait;
use uuid::Uuid;

use super::Planner;
use crate::types::{
    ComplexityLevel, EvidenceSnapshot, IRMetadata, IRNode, IRNodeKind, Intent,
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
        ComplexityLevel::Critical => StrategyKind::Consensus,
        ComplexityLevel::High => StrategyKind::Reflection,
        ComplexityLevel::Medium => StrategyKind::Single,
        ComplexityLevel::Low => StrategyKind::Single,
    }
}

fn select_model(requirements: &Requirements) -> String {
    match requirements.intent_classification {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_requirements(intent: Intent, complexity: ComplexityLevel) -> Requirements {
        Requirements {
            intent_classification: intent,
            complexity,
            has_files: false,
            context_window: 4096,
            original_text: String::new(),
            execution_intent: None,
            output_preferences: None,
            model_requirements: None,
        }
    }

    #[tokio::test]
    async fn test_simple_planner_returns_single_generate_node() {
        let planner = SimplePlanner;
        let reqs = make_requirements(Intent::General, ComplexityLevel::Low);
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 1);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
    }

    #[test]
    fn test_simple_planner_strategy_selection() {
        let critical = make_requirements(Intent::General, ComplexityLevel::Critical);
        assert_eq!(select_strategy(&critical), StrategyKind::Consensus);

        let high = make_requirements(Intent::General, ComplexityLevel::High);
        assert_eq!(select_strategy(&high), StrategyKind::Reflection);

        let medium = make_requirements(Intent::General, ComplexityLevel::Medium);
        assert_eq!(select_strategy(&medium), StrategyKind::Single);

        let low = make_requirements(Intent::General, ComplexityLevel::Low);
        assert_eq!(select_strategy(&low), StrategyKind::Single);
    }

    #[test]
    fn test_simple_planner_model_selection() {
        for intent in &[Intent::Code, Intent::Debug, Intent::Architecture, Intent::Analysis, Intent::Creative, Intent::General] {
            let reqs = make_requirements(intent.clone(), ComplexityLevel::Medium);
            let model = select_model(&reqs);
            assert!(!model.is_empty(), "Model should not be empty for {:?}", intent);
        }
    }

    #[tokio::test]
    async fn test_simple_planner_metadata() {
        let planner = SimplePlanner;
        let reqs = make_requirements(Intent::General, ComplexityLevel::Low);
        let ir = planner.plan(&reqs, &[], None).await;
        assert!(ir.metadata.estimated_cost > 0.0);
        assert!(ir.metadata.estimated_tokens > 0);
    }
}
