use fusion_core::WorkerId;
use fusion_worker_protocol::WorkerManifest;

pub struct WorkerDaemon {
    manifest: WorkerManifest,
}

impl WorkerDaemon {
    pub fn new(id: &str) -> Self {
        Self {
            manifest: WorkerManifest {
                id: WorkerId(id.to_string()),
                version: "0.14.0".to_string(),
                capabilities: vec!["chat".to_string(), "embeddings".to_string()],
                protocol_version: "v1".to_string(),
            },
        }
    }

    pub fn manifest(&self) -> &WorkerManifest {
        &self.manifest
    }
}
