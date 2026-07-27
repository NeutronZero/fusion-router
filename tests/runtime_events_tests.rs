use uuid::Uuid;
use fusion_router::events::bus::{BroadcastEventBus, EventBus};
use fusion_router::events::consumers::{
    CheckpointPolicy, CheckpointProjection, PersistentEventStoreProjection, TimelineProjection,
};
use fusion_router::events::payload::ExecutionEvent;
use fusion_router::events::projection::ProjectionDispatcher;
use fusion_router::events::ExecutionEventEnvelope;

#[tokio::test]
async fn test_end_to_end_runtime_event_pipeline() {
    let bus = BroadcastEventBus::default();
    let temp_dir = std::env::temp_dir().join(format!("fusion_e2e_events_{}", Uuid::new_v4()));

    let mut dispatcher = ProjectionDispatcher::new();
    dispatcher.register(TimelineProjection::new("exec-e2e-1"));
    dispatcher.register(CheckpointProjection::new(CheckpointPolicy::EveryNode, temp_dir.clone()));
    dispatcher.register(PersistentEventStoreProjection::new(temp_dir.clone()));

    let handle = dispatcher.spawn_listener(&bus);

    let env1 = ExecutionEventEnvelope::new(
        "wf-e2e",
        "exec-e2e-1",
        Some("corr-100".into()),
        1,
        None,
        ExecutionEvent::WorkflowStarted {
            intent: "Balanced".into(),
            input_tokens: 250,
        },
    );

    let env2 = ExecutionEventEnvelope::new(
        "wf-e2e",
        "exec-e2e-1",
        Some("corr-100".into()),
        2,
        Some(env1.event_id.clone()),
        ExecutionEvent::NodeFinished {
            node_id: "node_1".into(),
            duration_ms: 120,
            prompt_tokens: 50,
            completion_tokens: 75,
        },
    );

    let env3 = ExecutionEventEnvelope::new(
        "wf-e2e",
        "exec-e2e-1",
        Some("corr-100".into()),
        3,
        Some(env2.event_id.clone()),
        ExecutionEvent::WorkflowCompleted {
            total_duration_ms: 200,
            total_cost_usd: 0.002,
        },
    );

    bus.publish(env1).await.unwrap();
    bus.publish(env2).await.unwrap();
    bus.publish(env3).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    handle.abort();

    let store = PersistentEventStoreProjection::new(temp_dir.clone());
    let loaded = store.load_events("exec-e2e-1").unwrap();
    assert_eq!(loaded.len(), 3);
    assert_eq!(loaded[0].sequence_number, 1);
    assert_eq!(loaded[1].sequence_number, 2);
    assert_eq!(loaded[2].sequence_number, 3);
    assert_eq!(loaded[0].correlation_id, Some("corr-100".into()));

    let _ = std::fs::remove_dir_all(temp_dir);
}
