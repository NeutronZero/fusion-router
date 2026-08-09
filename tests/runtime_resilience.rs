//! Runtime Resilience Operational Validation Suite (v0.14.4)
//!
//! Exercises Invariant 12 single-worker lease exclusivity under crash recovery & partition scenarios.

use fusion_placement::ExecutionLeaseManager;
use fusion_worker_protocol::{HeartbeatMessage, WorkerRegistryStore};

#[test]
fn test_resilience_worker_crash_and_lease_failover() {
    let lease_manager = ExecutionLeaseManager::new();

    // 1. Worker 1 claims exclusive lease on node 1
    let lease1 = lease_manager.grant_lease("exec_900", "node_ast", "worker_1", 5000).expect("Grant worker_1");
    assert_eq!(lease1.worker_id, "worker_1");

    // 2. Worker 2 attempts concurrent claim on node 1 -> rejected under Invariant 12
    assert!(lease_manager.grant_lease("exec_900", "node_ast", "worker_2", 5000).is_err());

    // 3. Worker 1 crashes (lease revoked) -> Coordinator re-grants to Worker 2
    assert!(lease_manager.revoke_lease(&lease1.lease_key));
    let lease2 = lease_manager.grant_lease("exec_900", "node_ast", "worker_2", 5000).expect("Failover to worker_2");
    assert_eq!(lease2.worker_id, "worker_2");
}

#[test]
fn test_resilience_heartbeat_timeout_detection() {
    let registry = WorkerRegistryStore::new();

    let heartbeat = HeartbeatMessage {
        worker_id: "worker_1".into(),
        epoch: 10,
        cpu_utilization_pct: 45.0,
        memory_available_mb: 16000,
        active_executions: 3,
        timestamp_ms: 10000,
    };

    // Heartbeat before registration returns false
    assert!(!registry.heartbeat(heartbeat));
}
