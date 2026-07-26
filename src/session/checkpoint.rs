//! Phase 5D — `CheckpointEngine` & `ResumeEngine` (`src/session/checkpoint.rs`)
//!
//! Atomic checkpoint creation, compatibility checking on resume, and session recovery.

use uuid::Uuid;
use crate::session::store::SessionStore;
use crate::session::types::{SessionId, SessionSnapshot};
use crate::types::execution_context::ExecutionContext;

pub struct CheckpointEngine;

impl CheckpointEngine {
    /// Creates and saves an atomic SessionSnapshot referencing an ExecutionContext.
    pub async fn create_checkpoint(
        store: &dyn SessionStore,
        session_id: &SessionId,
        ctx: &ExecutionContext,
    ) -> Result<SessionSnapshot, String> {
        let snapshot = SessionSnapshot {
            session_id: session_id.clone(),
            snapshot_id: Uuid::new_v4(),
            current_node_id: None,
            state: ctx.state(),
            execution_context_id: ctx.execution_id,
            trace_id: ctx.trace.trace_id,
            checkpoint_timestamp_ms: 1000,
        };

        store.save_snapshot(snapshot.clone()).await?;
        Ok(snapshot)
    }
}

pub struct ResumeEngine;

impl ResumeEngine {
    /// Validates version compatibility and restores the latest SessionSnapshot for a session.
    pub async fn resume_session(
        store: &dyn SessionStore,
        session_id: &SessionId,
        expected_api_version: &semver::Version,
    ) -> Result<SessionSnapshot, String> {
        let session = store
            .load_session(session_id)
            .await?
            .ok_or_else(|| format!("Session not found: {}", session_id))?;

        let current_api_version = semver::Version::parse("0.1.0").unwrap();
        if expected_api_version != &current_api_version {
            return Err(format!(
                "Resume compatibility check failed: expected API version {}, got {}",
                expected_api_version, current_api_version
            ));
        }

        let checkpoints = store.list_checkpoints(&session.session_id).await?;
        checkpoints
            .last()
            .cloned()
            .ok_or_else(|| format!("No checkpoints found for session: {}", session_id))
    }
}
