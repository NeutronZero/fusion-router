use crate::intent::NormalizedIntent;
use fusion_ir::{WorkflowBuilder, WorkflowIR, WorkflowMetadata, ValidationError};
use fusion_core::NanoUSD;

pub fn intent_to_workflow(intent: &NormalizedIntent) -> Result<WorkflowIR, ValidationError> {
    let mut config = std::collections::BTreeMap::new();
    config.insert("goal".into(), serde_json::Value::String(intent.goal.clone()));
    config.insert("intent_kind".into(), serde_json::Value::String(format!("{:?}", intent.kind)));
    if let Some(latency) = intent.constraints.max_latency_ms {
        config.insert("max_latency_ms".into(), serde_json::Value::Number(latency.into()));
    }
    if let Some(tokens) = intent.constraints.max_tokens {
        config.insert("max_tokens".into(), serde_json::Value::Number(tokens.into()));
    }

    let estimated_cost = intent.budget.max_cost_usd
        .map(|usd| NanoUSD::from_nanos((usd * 1_000_000_000.0) as u64))
        .unwrap_or(NanoUSD::ZERO);

    let metadata = WorkflowMetadata {
        estimated_cost,
        estimated_tokens: intent.budget.max_tokens.unwrap_or(0),
        ..WorkflowMetadata::default()
    };

    let builder = WorkflowBuilder::new().metadata(metadata);
    let builder = builder.add_node("task", fusion_ir::WorkflowNodeKind::Task, None)?;
    let builder = builder.with_config("task", config)?;
    let builder = builder.output("output")?;
    let builder = builder.sequential("task", "output")?;
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::{Constraints, IntentKind, NormalizedIntent};
    use fusion_ir::WorkflowEdgeKind;
    use uuid::Uuid;

    fn intent_with(
        goal: &str,
        kind: IntentKind,
        constraints: Constraints,
        budget: crate::intent::Budget,
    ) -> NormalizedIntent {
        NormalizedIntent {
            intent_id: Uuid::new_v4(),
            goal: goal.into(),
            kind,
            constraints,
            budget,
            session_id: None,
        }
    }

    fn default_constraints() -> Constraints {
        Constraints {
            max_latency_ms: None,
            max_cost_usd: None,
            max_tokens: None,
            min_confidence: None,
        }
    }

    fn default_budget() -> crate::intent::Budget {
        crate::intent::Budget {
            max_cost_usd: None,
            max_tokens: None,
            max_execution_ms: None,
        }
    }

    #[test]
    fn test_lowering_builds_valid_chain() {
        let ir = intent_to_workflow(&intent_with(
            "summarize the docs",
            IntentKind::Analysis,
            default_constraints(),
            default_budget(),
        ))
        .expect("must lower without validation errors");

        let nodes = ir.nodes();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].id(), "task");
        assert_eq!(nodes[0].config()["goal"], "summarize the docs");
        assert_eq!(nodes[0].config()["intent_kind"], "Analysis");
        assert_eq!(nodes[1].id(), "output");
    }

    #[test]
    fn test_lowering_omits_absent_constraints() {
        let ir = intent_to_workflow(&intent_with(
            "goal",
            IntentKind::Code,
            default_constraints(),
            default_budget(),
        ))
        .unwrap();

        let config = ir.nodes()[0].config();
        assert!(!config.contains_key("max_latency_ms"));
        assert!(!config.contains_key("max_tokens"));
    }

    #[test]
    fn test_lowering_includes_supplied_constraints() {
        let ir = intent_to_workflow(&intent_with(
            "goal",
            IntentKind::Code,
            Constraints {
                max_latency_ms: Some(250),
                max_cost_usd: Some(0.5),
                max_tokens: Some(4096),
                min_confidence: Some(0.8),
            },
            default_budget(),
        ))
        .unwrap();

        let config = ir.nodes()[0].config();
        assert_eq!(config["max_latency_ms"], 250);
        assert_eq!(config["max_tokens"], 4096);
    }

    #[test]
    fn test_lowering_wires_sequential_edge() {
        let ir = intent_to_workflow(&intent_with(
            "goal",
            IntentKind::Analysis,
            default_constraints(),
            default_budget(),
        ))
        .unwrap();

        assert!(ir.edges().iter().any(|e| {
            e.from() == "task" && e.to() == "output" && e.kind() == WorkflowEdgeKind::Sequential
        }));
    }

    #[test]
    fn test_lowering_carries_budget_metadata() {
        let ir = intent_to_workflow(&intent_with(
            "goal",
            IntentKind::Analysis,
            default_constraints(),
            crate::intent::Budget {
                max_cost_usd: Some(1.5),
                max_tokens: Some(8000),
                max_execution_ms: None,
            },
        ))
        .unwrap();

        assert_eq!(ir.metadata().estimated_cost, NanoUSD::from_nanos(1_500_000_000));
        assert_eq!(ir.metadata().estimated_tokens, 8000);
    }
}
