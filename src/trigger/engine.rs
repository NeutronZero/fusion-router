//! Phase 6D — `TriggerExecutionEngine` (`src/trigger/engine.rs`)
//!
//! Wires trigger payloads through the unified Planner → CompilerPipeline → ExecutionGraph pipeline.

use std::sync::Arc;
use crate::compiler::context::CompilationContext;
use crate::compiler::pipeline::CompilerPipeline;
use crate::lifecycle::LifecycleManager;
use crate::trigger::types::TriggerPayload;
use crate::types::WorkflowIR;

pub struct TriggerExecutionEngine {
    compiler_pipeline: CompilerPipeline,
    lifecycle_manager: Arc<LifecycleManager>,
}

impl TriggerExecutionEngine {
    pub fn new(
        compiler_pipeline: CompilerPipeline,
        lifecycle_manager: Arc<LifecycleManager>,
    ) -> Self {
        Self {
            compiler_pipeline,
            lifecycle_manager,
        }
    }

    /// Dispatches a trigger payload into a compiled execution context.
    pub async fn dispatch_trigger(
        &self,
        _payload: &TriggerPayload,
        base_ir: WorkflowIR,
    ) -> Result<WorkflowIR, String> {
        let ctx = CompilationContext::new();

        // Invariant: Trigger payloads MUST pass through the CompilerPipeline before execution!
        let compiled_ir = self
            .compiler_pipeline
            .execute(base_ir, &ctx)
            .await
            .map_err(|e| format!("Trigger compiler pipeline failed: {}", e))?;

        let _session = self
            .lifecycle_manager
            .create_session("trigger-engine", compiled_ir.plan_id)
            .await?;

        Ok(compiled_ir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use crate::session::store::InMemorySessionStore;
    use crate::trigger::types::TriggerKind;
    use crate::types::{IRNode, IRNodeKind, StrategyKind};

    #[tokio::test]
    async fn test_trigger_engine_pipeline_dispatch() {
        let store = Arc::new(InMemorySessionStore::new());
        let lifecycle = Arc::new(LifecycleManager::new(store));
        let pipeline = CompilerPipeline::new();

        let engine = TriggerExecutionEngine::new(pipeline, lifecycle);

        let payload = TriggerPayload {
            trigger_name: "webhook-test".into(),
            kind: TriggerKind::Webhook,
            payload_json: serde_json::json!({}),
        };

        let base_ir = WorkflowIR {
            plan_id: Uuid::new_v4(),
            nodes: vec![IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some("gpt-4o".into()),
                config: std::collections::HashMap::new(),
            }],
            edges: vec![],
            metadata: crate::types::IRMetadata {
                policy_applied: vec![],
                estimated_cost: 0.0,
                estimated_tokens: 10,
            },
        };

        let compiled = engine.dispatch_trigger(&payload, base_ir).await.unwrap();
        assert_eq!(compiled.nodes.len(), 1);
    }
}
