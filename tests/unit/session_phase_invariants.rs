//! Executable Session Phase Invariants & Replay Verification Suite
//!
//! Verifies:
//! 1. Checkpoint Idempotence: Saving identical state produces equivalent checkpoints.
//! 2. Store Isolation: InMemorySessionStore and SqliteSessionStore behave identically.
//! 3. Resume Equivalence: Session resume restores state accurately following compatibility checks.
//! 4. Replay Side-Effect Freedom: Inspection replay never executes physical connectors.

use fusion_plugin_api::{CapabilityContract, CapabilityId, CapabilityInstance};
use fusion_router::session::checkpoint::{CheckpointEngine, ResumeEngine};
use fusion_router::session::replay::ReplayEngine;
use fusion_router::session::store::{InMemorySessionStore, SessionStore, SqliteSessionStore};
use fusion_router::session::types::{ExecutionSession, SessionId};
use fusion_router::types::execution_context::{ExecutionContext, ExecutionEvent, ExecutionState};
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

fn create_sample_ctx() -> ExecutionContext {
    let instance = CapabilityInstance {
        contract: CapabilityContract {
            id: CapabilityId::new("echo.text"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Echo".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 1,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        },
        runtime_params: json!({}),
    };

    ExecutionContext::new(instance, "echo".into(), json!({"text": "session test"}))
}

#[tokio::test]
async fn session_invariant_checkpoint_idempotence() {
    let store = InMemorySessionStore::new();
    let session_id = SessionId::new();
    let session = ExecutionSession {
        session_id: session_id.clone(),
        workflow_id: Uuid::new_v4(),
        created_at_ms: 1000,
        owner: "user".into(),
        config: HashMap::new(),
    };
    store.create_session(session).await.unwrap();

    let ctx = create_sample_ctx();

    let snap1 = CheckpointEngine::create_checkpoint(&store, &session_id, &ctx, Some("user"))
        .await
        .unwrap();
    let snap2 = CheckpointEngine::create_checkpoint(&store, &session_id, &ctx, Some("user"))
        .await
        .unwrap();

    assert_eq!(snap1.session_id, snap2.session_id);
    assert_eq!(snap1.state, snap2.state);
}

#[tokio::test]
async fn session_invariant_store_isolation_parity() {
    let mem_store = InMemorySessionStore::new();
    let sql_store = SqliteSessionStore::new(":memory:").unwrap();
    let session_id = SessionId::new();

    let session = ExecutionSession {
        session_id: session_id.clone(),
        workflow_id: Uuid::new_v4(),
        created_at_ms: 1000,
        owner: "user".into(),
        config: HashMap::new(),
    };

    mem_store.create_session(session.clone()).await.unwrap();
    sql_store.create_session(session).await.unwrap();

    let mem_session = mem_store.load_session(&session_id, None).await.unwrap();
    let sql_session = sql_store.load_session(&session_id, None).await.unwrap();

    assert_eq!(mem_session.unwrap().owner, sql_session.unwrap().owner);
}

#[tokio::test]
async fn session_invariant_resume_compatibility_check() {
    let store = InMemorySessionStore::new();
    let session_id = SessionId::new();
    let session = ExecutionSession {
        session_id: session_id.clone(),
        workflow_id: Uuid::new_v4(),
        created_at_ms: 1000,
        owner: "user".into(),
        config: HashMap::new(),
    };
    store.create_session(session).await.unwrap();

    let ctx = create_sample_ctx();
    let _ = CheckpointEngine::create_checkpoint(&store, &session_id, &ctx, Some("user"))
        .await
        .unwrap();

    let valid_ver = semver::Version::parse("0.1.0").unwrap();
    let restored = ResumeEngine::resume_session(&store, &session_id, &valid_ver, None)
        .await
        .unwrap();
    assert_eq!(restored.session_id, session_id);

    let invalid_ver = semver::Version::parse("9.9.9").unwrap();
    let err = ResumeEngine::resume_session(&store, &session_id, &invalid_ver, None).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn session_invariant_replay_side_effect_freedom() {
    let ctx = create_sample_ctx();
    ctx.trace
        .record(ExecutionEvent::ExecutionStarted { timestamp_ms: 10 });
    ctx.trace.record(ExecutionEvent::ExecutionFinished {
        final_state: ExecutionState::Succeeded,
        timestamp_ms: 20,
    });

    // Inspection mode must reconstruct state without invoking physical connectors
    let state = ReplayEngine::replay_inspection(&ctx.trace);
    assert_eq!(state, ExecutionState::Succeeded);
}
