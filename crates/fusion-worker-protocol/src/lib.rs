use fusion_core::WorkerId;
use fusion_placement::{WorkerCapabilities, WorkerStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerManifest {
    pub id: WorkerId,
    pub version: String,
    pub capabilities: WorkerCapabilities,
    pub protocol_version: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterNodeInfo {
    pub worker_id: String,
    pub hostname: String,
    pub capabilities: WorkerCapabilities,
    pub status: WorkerStatus,
    pub is_online: bool,
    pub registered_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatMessage {
    pub worker_id: String,
    pub epoch: u64,
    pub cpu_utilization_pct: f32,
    pub memory_available_mb: u64,
    pub active_executions: u32,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct WorkerRegistryStore {
    nodes: Arc<RwLock<HashMap<String, ClusterNodeInfo>>>,
}

impl WorkerRegistryStore {
    pub fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register(&self, info: ClusterNodeInfo) {
        let mut map = self.nodes.write().unwrap();
        map.insert(info.worker_id.clone(), info);
    }

    pub fn heartbeat(&self, heartbeat: HeartbeatMessage) -> bool {
        let mut map = self.nodes.write().unwrap();
        if let Some(node) = map.get_mut(&heartbeat.worker_id) {
            node.status.cpu_utilization_pct = heartbeat.cpu_utilization_pct;
            node.status.memory_available_mb = heartbeat.memory_available_mb;
            node.status.active_executions = heartbeat.active_executions;
            node.status.last_heartbeat_at = chrono::Utc::now().to_rfc3339();
            node.is_online = true;
            true
        } else {
            false
        }
    }

    pub fn get_active_nodes(&self) -> Vec<ClusterNodeInfo> {
        let map = self.nodes.read().unwrap();
        map.values().filter(|n| n.is_online).cloned().collect()
    }

    pub fn evict_stale_workers(&self, timeout_secs: u64) -> usize {
        let mut map = self.nodes.write().unwrap();
        let now = chrono::Utc::now();
        let mut evicted = 0;

        for node in map.values_mut() {
            if !node.is_online {
                continue;
            }
            if let Ok(last_hb) = chrono::DateTime::parse_from_rfc3339(&node.status.last_heartbeat_at) {
                let elapsed = (now - last_hb.with_timezone(&chrono::Utc)).num_seconds();
                if elapsed > timeout_secs as i64 {
                    node.is_online = false;
                    evicted += 1;
                }
            }
        }
        evicted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_registry_lifecycle() {
        let registry = WorkerRegistryStore::new();

        let capabilities = WorkerCapabilities {
            llm_models: vec!["gpt-4o".into()],
            memory_mb: 32768,
            has_gpu: true,
            tools: vec!["python_interpreter".into()],
            max_parallelism: 16,
            locality_zone: "us-east-1a".into(),
            labels: HashMap::new(),
            protocol_version: 1,
        };

        let status = WorkerStatus {
            worker_id: "w1".into(),
            cpu_utilization_pct: 12.5,
            memory_available_mb: 28000,
            active_executions: 1,
            health_score: 0.99,
            last_heartbeat_at: chrono::Utc::now().to_rfc3339(),
        };

        let info = ClusterNodeInfo {
            worker_id: "w1".into(),
            hostname: "worker-1.internal".into(),
            capabilities,
            status,
            is_online: true,
            registered_at: chrono::Utc::now().to_rfc3339(),
        };

        registry.register(info);
        assert_eq!(registry.get_active_nodes().len(), 1);

        let heartbeat = HeartbeatMessage {
            worker_id: "w1".into(),
            epoch: 1,
            cpu_utilization_pct: 18.2,
            memory_available_mb: 26500,
            active_executions: 2,
            timestamp_ms: 1000,
        };

        assert!(registry.heartbeat(heartbeat));
        let nodes = registry.get_active_nodes();
        assert_eq!(nodes[0].status.active_executions, 2);
    }
}
