//! Phase 5C — `InMemorySessionStore` (`src/session/store/memory.rs`)

use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use super::SessionStore;
use crate::session::types::{ExecutionSession, SessionId, SessionSnapshot};

#[derive(Clone)]
pub struct InMemorySessionStore {
    sessions: Arc<RwLock<HashMap<SessionId, ExecutionSession>>>,
    snapshots: Arc<RwLock<HashMap<SessionId, Vec<SessionSnapshot>>>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            snapshots: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn create_session(&self, session: ExecutionSession) -> Result<(), String> {
        let mut guard = self.sessions.write();
        guard.insert(session.session_id.clone(), session);
        Ok(())
    }

    async fn load_session(
        &self,
        session_id: &SessionId,
        owner: Option<&str>,
    ) -> Result<Option<ExecutionSession>, String> {
        let guard = self.sessions.read();
        let session = guard.get(session_id).cloned();
        match (session, owner) {
            (Some(s), Some(want)) if s.owner != want => Ok(None),
            (other, _) => Ok(other),
        }
    }

    async fn save_snapshot(&self, snapshot: SessionSnapshot) -> Result<(), String> {
        let mut guard = self.snapshots.write();
        guard
            .entry(snapshot.session_id.clone())
            .or_default()
            .push(snapshot);
        Ok(())
    }

    async fn list_checkpoints(
        &self,
        session_id: &SessionId,
        owner: Option<&str>,
    ) -> Result<Vec<SessionSnapshot>, String> {
        if let Some(want) = owner {
            let s_guard = self.sessions.read();
            match s_guard.get(session_id) {
                Some(s) if s.owner != want => {
                    return Err(format!("session not found: {}", session_id))
                }
                None => return Err(format!("session not found: {}", session_id)),
                _ => {}
            }
        }
        let guard = self.snapshots.read();
        Ok(guard.get(session_id).cloned().unwrap_or_default())
    }

    async fn delete_session(
        &self,
        session_id: &SessionId,
        owner: Option<&str>,
    ) -> Result<(), String> {
        if let Some(want) = owner {
            let s_guard = self.sessions.read();
            match s_guard.get(session_id) {
                Some(s) if s.owner != want => {
                    return Err(format!("session not found: {}", session_id))
                }
                None => return Err(format!("session not found: {}", session_id)),
                _ => {}
            }
        }
        let mut s_guard = self.sessions.write();
        let mut snap_guard = self.snapshots.write();
        s_guard.remove(session_id);
        snap_guard.remove(session_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_session_store_crud() {
        let store = InMemorySessionStore::new();
        let session_id = SessionId::new();

        let session = ExecutionSession {
            session_id: session_id.clone(),
            workflow_id: uuid::Uuid::new_v4(),
            created_at_ms: 100,
            owner: "test".into(),
            config: HashMap::new(),
        };

        store.create_session(session).await.unwrap();
        let loaded = store.load_session(&session_id, None).await.unwrap();
        assert!(loaded.is_some());

        let snapshot = SessionSnapshot {
            session_id: session_id.clone(),
            snapshot_id: uuid::Uuid::new_v4(),
            current_node_id: None,
            state: crate::types::execution_context::ExecutionState::Succeeded,
            execution_context_id: uuid::Uuid::new_v4(),
            trace_id: uuid::Uuid::new_v4(),
            checkpoint_timestamp_ms: 105,
        };

        store.save_snapshot(snapshot).await.unwrap();
        let checkpoints = store.list_checkpoints(&session_id, None).await.unwrap();
        assert_eq!(checkpoints.len(), 1);
    }
}
