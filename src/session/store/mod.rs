//! Phase 5C — `SessionStore` Trait & Backends (`src/session/store/mod.rs`)
//!
//! Minimal storage engine contract for session persistence adhering to ADR-026 & ADR-030.

use crate::session::types::{ExecutionSession, SessionId, SessionSnapshot};
use async_trait::async_trait;

pub mod memory;
pub mod sqlite;

#[allow(unused_imports)]
pub use memory::InMemorySessionStore;
#[allow(unused_imports)]
pub use sqlite::SqliteSessionStore;

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn create_session(&self, session: ExecutionSession) -> Result<(), String>;
    async fn load_session(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<ExecutionSession>, String>;
    async fn save_snapshot(&self, snapshot: SessionSnapshot) -> Result<(), String>;
    async fn list_checkpoints(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<SessionSnapshot>, String>;
    async fn delete_session(&self, session_id: &SessionId) -> Result<(), String>;
}
