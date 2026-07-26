use async_trait::async_trait;
use crate::types::{EvidenceSnapshot, ExecutionRecord};

#[derive(Debug, Clone, PartialEq)]
pub struct ModelPerformanceStats {
    pub model: String,
    pub total_requests: u64,
    pub success_count: u64,
    pub avg_latency_ms: f64,
    pub avg_cost: f64,
}

#[async_trait]
pub trait EvidenceRepository: Send + Sync {
    async fn record(&self, entry: ExecutionRecord) -> anyhow::Result<()>;
    async fn snapshot(&self) -> anyhow::Result<EvidenceSnapshot>;
    async fn get_model_stats(&self, window_hours: u32) -> anyhow::Result<Vec<ModelPerformanceStats>>;
}

mod sqlite_repo;
pub use sqlite_repo::SqliteEvidenceRepository;

pub mod metrics;
pub mod stream_metrics;
pub mod connector_metrics;
pub mod audit;
pub mod tracing;
pub mod calibration;
pub mod unified_diagnostics;

