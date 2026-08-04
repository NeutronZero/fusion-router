use fusion_core::WorkerId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerManifest {
    pub id: WorkerId,
    pub version: String,
    pub capabilities: Vec<String>,
    pub protocol_version: String,
}
