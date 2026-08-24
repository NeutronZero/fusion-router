//! Phase 5B — `ExecutionSession` & `SessionSnapshot` (`src/session/types.rs`)
//!
//! Decouples static session identity from transient execution snapshots.

use crate::types::execution_context::ExecutionState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionStatus {
    Active,
    Paused,
    Completed,
    Failed,
    Terminated,
}

/// Static immutable identity and configuration of an execution session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSession {
    pub session_id: SessionId,
    pub workflow_id: Uuid,
    pub created_at_ms: u64,
    pub owner: String,
    pub config: HashMap<String, String>,
}

/// Transient execution snapshot capturing runtime state for persistence and checkpointing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: SessionId,
    pub snapshot_id: Uuid,
    pub current_node_id: Option<Uuid>,
    pub state: ExecutionState,
    pub execution_context_id: Uuid,
    pub trace_id: Uuid,
    pub checkpoint_timestamp_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_session_and_snapshot_creation() {
        let session = ExecutionSession {
            session_id: SessionId::new(),
            workflow_id: Uuid::new_v4(),
            created_at_ms: 1000,
            owner: "admin".into(),
            config: HashMap::new(),
        };

        let snapshot = SessionSnapshot {
            session_id: session.session_id.clone(),
            snapshot_id: Uuid::new_v4(),
            current_node_id: Some(Uuid::new_v4()),
            state: ExecutionState::Running,
            execution_context_id: Uuid::new_v4(),
            trace_id: Uuid::new_v4(),
            checkpoint_timestamp_ms: 1005,
        };

        assert_eq!(snapshot.session_id, session.session_id);
        assert_eq!(snapshot.state, ExecutionState::Running);
    }
}
