//! Phase 6A — `LifecycleManager` (`src/lifecycle/manager.rs`)
//!
//! Non-executing orchestration engine managing sessions, checkpointing, pause/resume, and scheduler handoffs.

use crate::session::checkpoint::CheckpointEngine;
use crate::session::store::SessionStore;
use crate::session::types::{ExecutionSession, SessionId, SessionSnapshot};
use crate::types::execution_context::ExecutionContext;
use std::sync::Arc;
use uuid::Uuid;

pub struct LifecycleManager {
    session_store: Arc<dyn SessionStore>,
}

impl LifecycleManager {
    pub fn new(session_store: Arc<dyn SessionStore>) -> Self {
        Self { session_store }
    }

    /// Creates a new execution session.
    pub async fn create_session(
        &self,
        owner: impl Into<String>,
        workflow_id: Uuid,
    ) -> Result<ExecutionSession, String> {
        let session = ExecutionSession {
            session_id: SessionId::new(),
            workflow_id,
            // Real wall-clock time (epoch ms): a constant stamp broke
            // created-at ordering and made sessions indistinguishable.
            created_at_ms: crate::session::checkpoint::now_epoch_ms(),
            owner: owner.into(),
            config: std::collections::HashMap::new(),
        };

        self.session_store.create_session(session.clone()).await?;
        Ok(session)
    }

    /// Records an atomic checkpoint for an active session.
    pub async fn checkpoint_session(
        &self,
        session_id: &SessionId,
        ctx: &ExecutionContext,
        owner: Option<&str>,
    ) -> Result<SessionSnapshot, String> {
        CheckpointEngine::create_checkpoint(self.session_store.as_ref(), session_id, ctx, owner)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::store::InMemorySessionStore;

    #[tokio::test]
    async fn test_lifecycle_manager_session_creation() {
        let store = Arc::new(InMemorySessionStore::new());
        let manager = LifecycleManager::new(store);

        let session = manager
            .create_session("admin", Uuid::new_v4())
            .await
            .unwrap();

        assert_eq!(session.owner, "admin");
    }

    #[tokio::test]
    async fn test_created_at_ms_is_real_wall_clock_and_monotonic() {
        let store = Arc::new(InMemorySessionStore::new());
        let manager = LifecycleManager::new(store);

        let first = manager.create_session("a", Uuid::new_v4()).await.unwrap();
        // Small sleep guarantees strictly increasing stamps across sessions.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let second = manager.create_session("b", Uuid::new_v4()).await.unwrap();

        assert!(
            second.created_at_ms >= first.created_at_ms,
            "timestamps must be monotonic: {} -> {}",
            first.created_at_ms,
            second.created_at_ms
        );
        // Sanity floor (~Sep 2020) proves a real clock, not a constant.
        assert!(first.created_at_ms > 1_600_000_000_000);
        let now = crate::session::checkpoint::now_epoch_ms();
        let drift = now.abs_diff(second.created_at_ms);
        assert!(
            drift < 60_000,
            "created_at_ms must track wall clock (drift {drift}ms)"
        );
    }

    #[test]
    fn test_now_epoch_ms_is_monotonic_per_call() {
        let t1 = crate::session::checkpoint::now_epoch_ms();
        let t2 = crate::session::checkpoint::now_epoch_ms();
        assert!(t2 >= t1, "clock must not go backwards");
        assert!(t1 > 1_600_000_000_000, "must be real epoch millis");
    }
}
