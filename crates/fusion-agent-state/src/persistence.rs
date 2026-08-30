//! Persistent `StateStore` for process-restart recovery.
//!
//! Backed by SQLite (same engine as `fusion_telemetry.db` and
//! `src/session/store/sqlite.rs`). Keeps the `Σ` canonical while `EventLog`
//! stays out-of-band. Optional feature ` persistence` to avoid hard dep in tests.

use crate::{ExecutionState, SkillSpec, StateError, StatePatch, StateStore};
use std::path::Path;

/// SQLite-backed store. Table `agent_state(id INTEGER PRIMARY KEY, value TEXT)`.
/// Single row (id=1) holds the canonical Σ as JSON. `commit()` is a transaction:
/// validate → merge → validate → `REPLACE`.
pub struct SqliteStateStore {
    skill: SkillSpec,
    conn: rusqlite::Connection,
}

impl SqliteStateStore {
    pub fn open(skill: SkillSpec, path: impl AsRef<Path>, initial: ExecutionState) -> Result<Self, StateError> {
        let conn = rusqlite::Connection::open(path.as_ref())
            .map_err(|e| StateError::PatchValidation(format!("sqlite open: {e}")))?;
        // Match repo SQLite hardening: WAL + busy_timeout + FK (see sqlite_repo.rs / session/store/sqlite.rs)
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;")
            .map_err(|e| StateError::PatchValidation(format!("pragma: {e}")))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_state (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .map_err(|e| StateError::PatchValidation(format!("create table: {e}")))?;

        // Validate skill provenance + initial against schema
        skill.validate()?;
        crate::validate_against_schema(&initial.value, &skill.schema)?;

        // Insert initial if empty
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM agent_state WHERE id=1", [], |r| r.get(0))
            .map_err(|e| StateError::PatchValidation(format!("count: {e}")))?;
        if count == 0 {
            let json = serde_json::to_string(&initial.value).map_err(|e| StateError::PatchValidation(e.to_string()))?;
            conn.execute("INSERT INTO agent_state (id, value) VALUES (1, ?1)", rusqlite::params![json])
                .map_err(|e| StateError::PatchValidation(format!("insert: {e}")))?;
        } else {
            // Validate existing row against current schema (fail closed on drift)
            let existing = Self::load_inner(&conn)?;
            crate::validate_against_schema(&existing.value, &skill.schema).map_err(|e| {
                StateError::SchemaViolation(format!("existing state violates new schema: {e}"))
            })?;
        }

        Ok(Self { skill, conn })
    }

    pub fn open_in_memory(skill: SkillSpec, initial: ExecutionState) -> Result<Self, StateError> {
        Self::open(skill, ":memory:", initial)
    }

    fn load_inner(conn: &rusqlite::Connection) -> Result<ExecutionState, StateError> {
        let json: String = conn
            .query_row("SELECT value FROM agent_state WHERE id=1", [], |r| r.get(0))
            .map_err(|e| StateError::PatchValidation(format!("load: {e}")))?;
        let value: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| StateError::PatchValidation(format!("json parse: {e}")))?;
        ExecutionState::new(value)
    }
}

impl StateStore for SqliteStateStore {
    fn load(&self) -> ExecutionState {
        Self::load_inner(&self.conn).expect("sqlite load must succeed after init")
    }

    fn validate_patch(&self, patch: &StatePatch) -> Result<(), StateError> {
        let cur = self.load();
        let merged = crate::merge_state(&cur.value, &patch.value)?;
        crate::validate_against_schema(&merged, &self.skill.schema)
    }

    fn commit(&mut self, patch: &StatePatch) -> Result<ExecutionState, StateError> {
        // Explicit transaction: load→merge→validate→REPLACE atomic under BEGIN IMMEDIATE
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| StateError::PatchValidation(format!("begin tx: {e}")))?;
        let cur = Self::load_inner(&tx)?;
        let merged = crate::merge_state(&cur.value, &patch.value)?;
        crate::validate_against_schema(&merged, &self.skill.schema)?;
        let json = serde_json::to_string(&merged).map_err(|e| StateError::PatchValidation(e.to_string()))?;
        tx.execute("REPLACE INTO agent_state (id, value) VALUES (1, ?1)", rusqlite::params![json])
            .map_err(|e| StateError::PatchValidation(format!("commit: {e}")))?;
        tx.commit()
            .map_err(|e| StateError::PatchValidation(format!("commit tx: {e}")))?;
        ExecutionState::new(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SkillSpec;
    use serde_json::json;

    fn skill() -> SkillSpec {
        SkillSpec::new(json!({"type": "object", "properties": {"counter": {"type": "integer"}}, "additionalProperties": true}), "x", "1")
    }

    #[test]
    fn sqlite_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let initial = ExecutionState::new(json!({"counter": 0})).unwrap();

        // First open
        let mut store = SqliteStateStore::open(skill(), &path, initial.clone()).unwrap();
        store.commit(&StatePatch::new(json!({"counter": 42})).unwrap()).unwrap();
        drop(store);

        // Reopen — state survived restart
        let store2 = SqliteStateStore::open(skill(), &path, initial).unwrap();
        assert_eq!(store2.load().value.get("counter").and_then(|v| v.as_u64()), Some(42));

        // Invalid patch still atomic
        let mut store3 = SqliteStateStore::open(skill(), &path, ExecutionState::new(json!({"counter": 0})).unwrap()).unwrap();
        let before = store3.load().value.clone();
        let bad = StatePatch::new(json!({"counter": {"bad": 1}})).unwrap();
        assert!(store3.commit(&bad).is_err());
        assert_eq!(store3.load().value, before);
    }

    #[test]
    fn sqlite_in_memory_works() {
        let mut s = SqliteStateStore::open_in_memory(skill(), ExecutionState::new(json!({"counter": 0})).unwrap()).unwrap();
        s.commit(&StatePatch::new(json!({"counter": 1})).unwrap()).unwrap();
        assert_eq!(s.load().value, json!({"counter": 1}));
    }
}
