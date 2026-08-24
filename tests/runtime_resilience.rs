//! Runtime Resilience Operational Validation Suite
//!
//! Exercises Invariant 12 single-worker lease exclusivity under crash recovery,
//! using the lease manager preserved in `fusion_scheduler::leases`.

use fusion_scheduler::leases::ExecutionLeaseManager;

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
fn test_resilience_lease_renewal_and_revoked_failover_epoch_monotonicity() {
    let lease_manager = ExecutionLeaseManager::new();

    let lease = lease_manager.grant_lease("exec_901", "node_ttl", "worker_a", 50_000).expect("grant");
    assert_eq!(lease.epoch, 1);
    assert!(lease_manager.renew_lease(&lease.lease_key), "live lease must renew");
    assert!(!lease.is_expired(lease.granted_at_ms));
    assert!(lease.is_expired(lease.granted_at_ms + 50_000), "TTL elapsed => expired");

    // Epoch must be monotonic per (exec,node) across crash/failover.
    assert!(lease_manager.revoke_lease(&lease.lease_key), "crash => revoke");
    let again = lease_manager.grant_lease("exec_901", "node_ttl", "worker_b", 50_000)
        .expect("revoked lease allows new owner");
    assert!(again.epoch > lease.epoch, "epoch must increase across owners");
}
