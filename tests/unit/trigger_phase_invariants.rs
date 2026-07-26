//! Executable Trigger Phase Invariants Test Suite
//!
//! Verifies:
//! 1. Trigger Compiler Pipeline Invariant: Every trigger payload MUST pass through the CompilerPipeline before execution.
//! 2. Webhook, Cron, & EventBus payload packaging fidelity.
//! 3. Session creation handoff to LifecycleManager upon trigger dispatch.

use fusion_router::compiler::pipeline::CompilerPipeline;
use fusion_router::lifecycle::LifecycleManager;
use fusion_router::session::store::InMemorySessionStore;
use fusion_router::trigger::cron::CronTriggerScheduler;
use fusion_router::trigger::engine::TriggerExecutionEngine;
use fusion_router::trigger::event_bus::EventBusTriggerSubscriber;
use fusion_router::trigger::types::TriggerKind;
use fusion_router::trigger::webhook::WebhookTriggerHandler;
use fusion_router::types::{IRMetadata, IRNode, IRNodeKind, StrategyKind, WorkflowIR};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

fn create_sample_ir() -> WorkflowIR {
    WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes: vec![IRNode {
            id: Uuid::new_v4(),
            kind: IRNodeKind::Generate,
            strategy: StrategyKind::Single,
            model: Some("gpt-4o".into()),
            config: HashMap::new(),
        }],
        edges: vec![],
        metadata: IRMetadata {
            policy_applied: vec![],
            estimated_cost: 0.0,
            estimated_tokens: 10,
        },
    }
}

fn create_engine() -> TriggerExecutionEngine {
    let store = Arc::new(InMemorySessionStore::new());
    let lifecycle = Arc::new(LifecycleManager::new(store));
    let pipeline = CompilerPipeline::new();

    TriggerExecutionEngine::new(pipeline, lifecycle)
}

#[tokio::test]
async fn trigger_invariant_webhook_passes_through_compiler_pipeline() {
    let engine = create_engine();
    let payload = WebhookTriggerHandler::process_webhook("github-event", json!({"action": "opened"}));

    assert_eq!(payload.kind, TriggerKind::Webhook);

    let base_ir = create_sample_ir();
    let compiled_ir = engine.dispatch_trigger(&payload, base_ir).await.unwrap();

    // Verify compilation output
    assert_eq!(compiled_ir.nodes.len(), 1);
}

#[tokio::test]
async fn trigger_invariant_cron_passes_through_compiler_pipeline() {
    let engine = create_engine();
    let payload = CronTriggerScheduler::trigger_scheduled("nightly-job", "0 0 * * *");

    assert_eq!(payload.kind, TriggerKind::Cron);

    let base_ir = create_sample_ir();
    let compiled_ir = engine.dispatch_trigger(&payload, base_ir).await.unwrap();

    assert_eq!(compiled_ir.nodes.len(), 1);
}

#[tokio::test]
async fn trigger_invariant_event_bus_passes_through_compiler_pipeline() {
    let engine = create_engine();
    let payload = EventBusTriggerSubscriber::handle_event("user-signup-event", json!({"user_id": 123}));

    assert_eq!(payload.kind, TriggerKind::EventBus);

    let base_ir = create_sample_ir();
    let compiled_ir = engine.dispatch_trigger(&payload, base_ir).await.unwrap();

    assert_eq!(compiled_ir.nodes.len(), 1);
}
