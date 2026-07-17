use async_trait::async_trait;
use crate::types::{EvidenceSnapshot, ExecutionRecord};

#[async_trait]
pub trait EvidenceRepository: Send + Sync {
    async fn record(&self, entry: ExecutionRecord) -> anyhow::Result<()>;
    async fn snapshot(&self) -> anyhow::Result<EvidenceSnapshot>;
}

mod sqlite_repo;
pub use sqlite_repo::SqliteEvidenceRepository;

pub mod metrics;
pub mod audit;
