//! Phase 5D — `CheckpointEngine` & `ResumeEngine` (`src/session/checkpoint.rs`)
//!
//! Atomic checkpoint creation, compatibility checking on resume, and session recovery.

use crate::session::store::SessionStore;
use crate::session::types::{SessionId, SessionSnapshot};
use crate::types::execution_context::ExecutionContext;
use uuid::Uuid;

/// Wall-clock epoch milliseconds. Used for checkpoint ordering; a single
/// helper so tests and callers share one time source.
pub fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub struct CheckpointEngine;

impl CheckpointEngine {
    /// Creates and saves an atomic SessionSnapshot referencing an ExecutionContext.
    pub async fn create_checkpoint(
        store: &dyn SessionStore,
        session_id: &SessionId,
        ctx: &ExecutionContext,
        owner: Option<&str>,
    ) -> Result<SessionSnapshot, String> {
        let snapshot = SessionSnapshot {
            session_id: session_id.clone(),
            snapshot_id: Uuid::new_v4(),
            current_node_id: None,
            state: ctx.state(),
            execution_context_id: ctx.execution_id,
            trace_id: ctx.trace.trace_id,
            // Real wall-clock time: constant stamps collapsed checkpoint
            // ordering (all checkpoints tied at the same millisecond).
            checkpoint_timestamp_ms: now_epoch_ms(),
        };

        store.save_snapshot(snapshot.clone(), owner).await?;
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
        owner: Option<&str>,
    ) -> Result<SessionSnapshot, String> {
        let session = store
            .load_session(session_id, owner)
            .await?
            .ok_or_else(|| format!("Session not found: {}", session_id))?;

        let current_api_version = semver::Version::new(0, 1, 0);
        if expected_api_version != &current_api_version {
            return Err(format!(
                "Resume compatibility check failed: expected API version {}, got {}",
                expected_api_version, current_api_version
            ));
        }

        let checkpoints = store.list_checkpoints(&session.session_id, owner).await?;
        checkpoints
            .last()
            .cloned()
            .ok_or_else(|| format!("No checkpoints found for session: {}", session_id))
    }
}
