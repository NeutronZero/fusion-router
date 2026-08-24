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
            created_at_ms: 1000,
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
    ) -> Result<SessionSnapshot, String> {
        CheckpointEngine::create_checkpoint(self.session_store.as_ref(), session_id, ctx).await
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
}
