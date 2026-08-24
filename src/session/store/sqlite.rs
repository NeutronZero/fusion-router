//! Phase 5C — `SqliteSessionStore` (`src/session/store/sqlite.rs`)
//!
//! Persistent SQLite backend for session storage. Implements `SessionStore`
//! with real SQLite persistence, replacing the earlier in-memory stub.

use async_trait::async_trait;
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::sync::Arc;

use super::SessionStore;
use crate::session::types::{ExecutionSession, SessionId, SessionSnapshot};

const CREATE_SESSIONS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    workflow_id TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    owner TEXT NOT NULL,
    config TEXT NOT NULL
)";

const CREATE_SNAPSHOTS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS snapshots (
    snapshot_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    current_node_id TEXT,
    state TEXT NOT NULL,
    execution_context_id TEXT NOT NULL,
    trace_id TEXT NOT NULL,
    checkpoint_timestamp_ms INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
)";

const CREATE_INDEX: &str = "
CREATE INDEX IF NOT EXISTS idx_snapshots_session ON snapshots(session_id)";

/// Persistent SQLite SessionStore backend.
#[derive(Clone)]
pub struct SqliteSessionStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteSessionStore {
    /// Opens (or creates) a SQLite database at the given path.
    /// Use `":memory:"` for an in-memory database (useful for tests).
    pub fn new(path: &str) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("failed to open sqlite session store at '{}': {}", path, e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("failed to set pragmas: {}", e))?;

        conn.execute_batch(CREATE_SESSIONS_TABLE)
            .map_err(|e| format!("failed to create sessions table: {}", e))?;
        conn.execute_batch(CREATE_SNAPSHOTS_TABLE)
            .map_err(|e| format!("failed to create snapshots table: {}", e))?;
        conn.execute_batch(CREATE_INDEX)
            .map_err(|e| format!("failed to create index: {}", e))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn create_session(&self, session: ExecutionSession) -> Result<(), String> {
        let conn = self.conn.lock();
        let config_json = serde_json::to_string(&session.config)
            .map_err(|e| format!("failed to serialize config: {}", e))?;
        conn.execute(
            "INSERT INTO sessions (session_id, workflow_id, created_at_ms, owner, config) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                session.session_id.to_string(),
                session.workflow_id.to_string(),
                session.created_at_ms as i64,
                session.owner,
                config_json,
            ],
        )
        .map_err(|e| format!("failed to insert session: {}", e))?;
        Ok(())
    }

    async fn load_session(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<ExecutionSession>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT session_id, workflow_id, created_at_ms, owner, config FROM sessions WHERE session_id = ?1")
            .map_err(|e| format!("failed to prepare load_session: {}", e))?;

        let result = stmt
            .query_row(params![session_id.to_string()], |row| {
                let session_id_str: String = row.get(0)?;
                let workflow_id_str: String = row.get(1)?;
                let created_at_ms: i64 = row.get(2)?;
                let owner: String = row.get(3)?;
                let config_json: String = row.get(4)?;
                Ok((
                    session_id_str,
                    workflow_id_str,
                    created_at_ms,
                    owner,
                    config_json,
                ))
            })
            .ok();

        match result {
            Some((sid, wf, ts, owner, cfg_json)) => {
                let session_id = SessionId(
                    uuid::Uuid::parse_str(&sid).map_err(|e| format!("bad session_id: {}", e))?,
                );
                let workflow_id =
                    uuid::Uuid::parse_str(&wf).map_err(|e| format!("bad workflow_id: {}", e))?;
                let config: std::collections::HashMap<String, String> =
                    serde_json::from_str(&cfg_json)
                        .map_err(|e| format!("failed to deserialize config: {}", e))?;
                Ok(Some(ExecutionSession {
                    session_id,
                    workflow_id,
                    created_at_ms: ts as u64,
                    owner,
                    config,
                }))
            }
            None => Ok(None),
        }
    }

    async fn save_snapshot(&self, snapshot: SessionSnapshot) -> Result<(), String> {
        let conn = self.conn.lock();
        let state_str = serde_json::to_string(&snapshot.state)
            .map_err(|e| format!("failed to serialize state: {}", e))?;
        conn.execute(
            "INSERT INTO snapshots (snapshot_id, session_id, current_node_id, state, execution_context_id, trace_id, checkpoint_timestamp_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                snapshot.snapshot_id.to_string(),
                snapshot.session_id.to_string(),
                snapshot.current_node_id.map(|u| u.to_string()),
                state_str,
                snapshot.execution_context_id.to_string(),
                snapshot.trace_id.to_string(),
                snapshot.checkpoint_timestamp_ms as i64,
            ],
        )
        .map_err(|e| format!("failed to insert snapshot: {}", e))?;
        Ok(())
    }

    async fn list_checkpoints(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<SessionSnapshot>, String> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT snapshot_id, session_id, current_node_id, state, execution_context_id, trace_id, checkpoint_timestamp_ms FROM snapshots WHERE session_id = ?1 ORDER BY checkpoint_timestamp_ms ASC")
            .map_err(|e| format!("failed to prepare list_checkpoints: {}", e))?;

        let rows = stmt
            .query_map(params![session_id.to_string()], |row| {
                let snapshot_id_str: String = row.get(0)?;
                let session_id_str: String = row.get(1)?;
                let current_node_id_str: Option<String> = row.get(2)?;
                let state_json: String = row.get(3)?;
                let exec_ctx_str: String = row.get(4)?;
                let trace_id_str: String = row.get(5)?;
                let checkpoint_ts: i64 = row.get(6)?;
                Ok((
                    snapshot_id_str,
                    session_id_str,
                    current_node_id_str,
                    state_json,
                    exec_ctx_str,
                    trace_id_str,
                    checkpoint_ts,
                ))
            })
            .map_err(|e| format!("failed to query snapshots: {}", e))?;

        let mut snapshots = Vec::new();
        for row in rows {
            let (sid, ssid, node_id, state_json, exec_ctx, trace_id, ts) =
                row.map_err(|e| format!("failed to read snapshot row: {}", e))?;

            let snapshot_id =
                uuid::Uuid::parse_str(&ssid).map_err(|e| format!("bad snapshot_id: {}", e))?;
            let session_id = SessionId(
                uuid::Uuid::parse_str(&sid).map_err(|e| format!("bad session_id: {}", e))?,
            );
            let current_node_id = node_id
                .map(|s| uuid::Uuid::parse_str(&s))
                .transpose()
                .map_err(|e| format!("bad current_node_id: {}", e))?;
            let state: crate::types::execution_context::ExecutionState =
                serde_json::from_str(&state_json)
                    .map_err(|e| format!("failed to deserialize state: {}", e))?;
            let execution_context_id = uuid::Uuid::parse_str(&exec_ctx)
                .map_err(|e| format!("bad execution_context_id: {}", e))?;
            let trace_id =
                uuid::Uuid::parse_str(&trace_id).map_err(|e| format!("bad trace_id: {}", e))?;

            snapshots.push(SessionSnapshot {
                session_id,
                snapshot_id,
                current_node_id,
                state,
                execution_context_id,
                trace_id,
                checkpoint_timestamp_ms: ts as u64,
            });
        }
        Ok(snapshots)
    }

    async fn delete_session(&self, session_id: &SessionId) -> Result<(), String> {
        let conn = self.conn.lock();
        let sid = session_id.to_string();
        conn.execute("DELETE FROM snapshots WHERE session_id = ?1", params![sid])
            .map_err(|e| format!("failed to delete snapshots: {}", e))?;
        conn.execute("DELETE FROM sessions WHERE session_id = ?1", params![sid])
            .map_err(|e| format!("failed to delete session: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::types::{SessionId, SessionSnapshot};
    use crate::types::execution_context::ExecutionState;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn test_session() -> ExecutionSession {
        ExecutionSession {
            session_id: SessionId::new(),
            workflow_id: Uuid::new_v4(),
            created_at_ms: 1000,
            owner: "test-user".into(),
            config: HashMap::from([("key".into(), "value".into())]),
        }
    }

    fn test_snapshot(session_id: &SessionId) -> SessionSnapshot {
        SessionSnapshot {
            session_id: session_id.clone(),
            snapshot_id: Uuid::new_v4(),
            current_node_id: Some(Uuid::new_v4()),
            state: ExecutionState::Running,
            execution_context_id: Uuid::new_v4(),
            trace_id: Uuid::new_v4(),
            checkpoint_timestamp_ms: 1005,
        }
    }

    #[tokio::test]
    async fn test_create_and_load_session() {
        let store = SqliteSessionStore::new(":memory:").unwrap();
        let session = test_session();
        let sid = session.session_id.clone();

        store.create_session(session.clone()).await.unwrap();
        let loaded = store.load_session(&sid).await.unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.session_id, sid);
        assert_eq!(loaded.owner, "test-user");
        assert_eq!(loaded.config.get("key").unwrap(), "value");
    }

    #[tokio::test]
    async fn test_load_nonexistent_session() {
        let store = SqliteSessionStore::new(":memory:").unwrap();
        let result = store.load_session(&SessionId::new()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_save_and_list_snapshots() {
        let store = SqliteSessionStore::new(":memory:").unwrap();
        let session = test_session();
        store.create_session(session.clone()).await.unwrap();

        let snap1 = test_snapshot(&session.session_id);
        let snap2 = SessionSnapshot {
            checkpoint_timestamp_ms: 1010,
            ..test_snapshot(&session.session_id)
        };

        store.save_snapshot(snap1).await.unwrap();
        store.save_snapshot(snap2).await.unwrap();

        let checkpoints = store.list_checkpoints(&session.session_id).await.unwrap();
        assert_eq!(checkpoints.len(), 2);
        assert_eq!(checkpoints[0].checkpoint_timestamp_ms, 1005);
        assert_eq!(checkpoints[1].checkpoint_timestamp_ms, 1010);
    }

    #[tokio::test]
    async fn test_delete_session_cascades() {
        let store = SqliteSessionStore::new(":memory:").unwrap();
        let session = test_session();
        store.create_session(session.clone()).await.unwrap();
        store
            .save_snapshot(test_snapshot(&session.session_id))
            .await
            .unwrap();

        store.delete_session(&session.session_id).await.unwrap();

        let loaded = store.load_session(&session.session_id).await.unwrap();
        assert!(loaded.is_none());

        let checkpoints = store.list_checkpoints(&session.session_id).await.unwrap();
        assert!(checkpoints.is_empty());
    }
}
