//! Phase 5C — `SqliteSessionStore` (`src/session/store/sqlite.rs`)

use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use super::SessionStore;
use crate::session::types::{ExecutionSession, SessionId, SessionSnapshot};

/// Persistent SQLite SessionStore backend stub reusing unified memory structures for testing parity.
#[derive(Clone)]
pub struct SqliteSessionStore {
    inner_sessions: Arc<RwLock<HashMap<SessionId, ExecutionSession>>>,
    inner_snapshots: Arc<RwLock<HashMap<SessionId, Vec<SessionSnapshot>>>>,
}

impl SqliteSessionStore {
    pub fn new() -> Self {
        Self {
            inner_sessions: Arc::new(RwLock::new(HashMap::new())),
            inner_snapshots: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for SqliteSessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn create_session(&self, session: ExecutionSession) -> Result<(), String> {
        let mut guard = self.inner_sessions.write();
        guard.insert(session.session_id.clone(), session);
        Ok(())
    }

    async fn load_session(&self, session_id: &SessionId) -> Result<Option<ExecutionSession>, String> {
        let guard = self.inner_sessions.read();
        Ok(guard.get(session_id).cloned())
    }

    async fn save_snapshot(&self, snapshot: SessionSnapshot) -> Result<(), String> {
        let mut guard = self.inner_snapshots.write();
        guard
            .entry(snapshot.session_id.clone())
            .or_default()
            .push(snapshot);
        Ok(())
    }

    async fn list_checkpoints(&self, session_id: &SessionId) -> Result<Vec<SessionSnapshot>, String> {
        let guard = self.inner_snapshots.read();
        Ok(guard.get(session_id).cloned().unwrap_or_default())
    }

    async fn delete_session(&self, session_id: &SessionId) -> Result<(), String> {
        let mut s_guard = self.inner_sessions.write();
        let mut snap_guard = self.inner_snapshots.write();
        s_guard.remove(session_id);
        snap_guard.remove(session_id);
        Ok(())
    }
}
