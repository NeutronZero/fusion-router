use fusion_router::session::checkpoint::CheckpointEngine;
use fusion_router::session::replay::ReplayEngine;
use fusion_router::session::store::{InMemorySessionStore, SessionStore};
use fusion_router::session::types::{ExecutionSession, SessionId, SessionSnapshot};
use fusion_router::types::execution_context::{ExecutionContext, ExecutionEvent, ExecutionState};
use fusion_plugin_api::{CapabilityContract, CapabilityId, CapabilityInstance};
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

#[tokio::test]
async fn test_replay_validation_from_checkpoint() {
    let instance = CapabilityInstance {
        contract: CapabilityContract {
            id: CapabilityId::new("replay.test"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Replay validation test".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 1,
            reliability_score: 1.0,
            supports_streaming: false,
        },
        runtime_params: json!({}),
    };

    let ctx = ExecutionContext::new(instance, "replay-test".into(), json!({"msg": "hello"}));
    ctx.set_state(ExecutionState::Succeeded);
    ctx.trace.record(ExecutionEvent::ExecutionStarted { timestamp_ms: 100 });
    ctx.trace.record(ExecutionEvent::PluginInvoked { plugin: "test-plugin".into() });
    ctx.trace.record(ExecutionEvent::PluginCompleted { status: "ok".into() });
    ctx.trace.record(ExecutionEvent::ExecutionFinished {
        final_state: ExecutionState::Succeeded,
        timestamp_ms: 200,
    });

    let store = InMemorySessionStore::new();
    let session_id = SessionId::new();
    let session = ExecutionSession {
        session_id: session_id.clone(),
        workflow_id: Uuid::new_v4(),
        created_at_ms: 100,
        owner: "replay-test".into(),
        config: HashMap::new(),
    };
    store.create_session(session).await.unwrap();

    let snapshot = CheckpointEngine::create_checkpoint(&store, &session_id, &ctx)
        .await
        .unwrap();

    let replayed_state = ReplayEngine::replay_inspection(&ctx.trace);
    assert_eq!(replayed_state, ExecutionState::Succeeded);
    assert_eq!(replayed_state, snapshot.state);
}

#[test]
fn test_snapshot_round_trip_serialization() {
    let snapshot = SessionSnapshot {
        session_id: SessionId::new(),
        snapshot_id: Uuid::new_v4(),
        current_node_id: Some(Uuid::new_v4()),
        state: ExecutionState::Running,
        execution_context_id: Uuid::new_v4(),
        trace_id: Uuid::new_v4(),
        checkpoint_timestamp_ms: 500,
    };

    let serialized = serde_json::to_string(&snapshot).unwrap();
    let deserialized: SessionSnapshot = serde_json::from_str(&serialized).unwrap();

    assert_eq!(deserialized.session_id, snapshot.session_id);
    assert_eq!(deserialized.snapshot_id, snapshot.snapshot_id);
    assert_eq!(deserialized.current_node_id, snapshot.current_node_id);
    assert_eq!(deserialized.state, ExecutionState::Running);
    assert_eq!(deserialized.execution_context_id, snapshot.execution_context_id);
    assert_eq!(deserialized.trace_id, snapshot.trace_id);
    assert_eq!(deserialized.checkpoint_timestamp_ms, 500);
}
