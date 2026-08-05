use fusion_core::WorkerId;
use fusion_worker_protocol::WorkerManifest;

use std::collections::HashMap;

pub struct WorkerDaemon {
    manifest: WorkerManifest,
}

impl WorkerDaemon {
    pub fn new(id: &str) -> Self {
        Self {
            manifest: WorkerManifest {
                id: WorkerId(id.to_string()),
                version: "0.14.0".to_string(),
                capabilities: fusion_placement::WorkerCapabilities {
                    llm_models: vec!["chat".to_string(), "embeddings".to_string()],
                    memory_mb: 16384,
                    has_gpu: true,
                    tools: vec![],
                    max_parallelism: 8,
                    locality_zone: "us-east-1a".into(),
                    labels: HashMap::new(),
                    protocol_version: 1,
                },
                protocol_version: 1,
            },
        }
    }

    pub fn manifest(&self) -> &WorkerManifest {
        &self.manifest
    }
}
