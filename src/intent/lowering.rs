use crate::intent::NormalizedIntent;
use fusion_ir::{WorkflowBuilder, WorkflowIR, WorkflowMetadata, ValidationError};

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

    let metadata = WorkflowMetadata {
        estimated_cost: intent.budget.max_cost_usd.unwrap_or(0.0),
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
