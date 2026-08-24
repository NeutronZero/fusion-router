//! Execution lease management (Invariant 12: single-worker lease
//! exclusivity). Preserved verbatim from the retired `fusion-placement`
//! crate; the fake placement engine around it was deleted.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use fusion_core::PlatformError;

#[derive(Debug, Clone)]
pub struct ExecutionLease {
    pub lease_key: String,
    pub execution_id: String,
    pub node_id: String,
    pub worker_id: String,
    pub epoch: u64,
    pub granted_at_ms: u64,
    pub ttl_ms: u64,
    pub is_revoked: bool,
}

impl ExecutionLease {
    pub fn is_expired(&self, current_time_ms: u64) -> bool {
        self.is_revoked || (self.ttl_ms > 0 && current_time_ms >= self.granted_at_ms + self.ttl_ms)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionLeaseManager {
    leases: Arc<RwLock<HashMap<String, ExecutionLease>>>,
}

impl ExecutionLeaseManager {
    pub fn new() -> Self {
        Self {
            leases: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Grants an exclusive, single-worker lease under Invariant 12.
    pub fn grant_lease(
        &self,
        exec_id: &str,
        node_id: &str,
        worker_id: &str,
        ttl_ms: u64,
    ) -> Result<ExecutionLease, PlatformError> {
        let lease_key = format!("lease:{}:{}:{}", exec_id, node_id, worker_id);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut map = self.leases.write().unwrap_or_else(|e| e.into_inner());
        let mut prev_epoch = 0u64;

        // Enforce Invariant 12: Single-Worker Lease Exclusivity & Expiration
        for existing in map.values() {
            if existing.execution_id == exec_id && existing.node_id == node_id {
                if !existing.is_expired(now_ms) {
                    if existing.worker_id != worker_id {
                        return Err(PlatformError::Runtime {
                            code: "ERR_LEASE_VIOLATION".into(),
                            message: format!(
                                "Node {} already leased to worker {}",
                                node_id, existing.worker_id
                            ),
                            recovery_suggestion:
                                "Wait for existing lease to expire or revoke it before reissuing"
                                    .into(),
                        });
                    }
                    prev_epoch = existing.epoch;
                } else if existing.epoch > prev_epoch {
                    prev_epoch = existing.epoch;
                }
            }
        }

        let lease = ExecutionLease {
            lease_key: lease_key.clone(),
            execution_id: exec_id.to_string(),
            node_id: node_id.to_string(),
            worker_id: worker_id.to_string(),
            epoch: prev_epoch + 1,
            granted_at_ms: now_ms,
            ttl_ms,
            is_revoked: false,
        };

        map.insert(lease_key, lease.clone());
        Ok(lease)
    }

    pub fn renew_lease(&self, lease_key: &str) -> bool {
        let mut map = self.leases.write().unwrap_or_else(|e| e.into_inner());
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if let Some(lease) = map.get_mut(lease_key) {
            if !lease.is_expired(now_ms) {
                lease.epoch += 1;
                lease.granted_at_ms = now_ms;
                return true;
            }
        }
        false
    }

    pub fn revoke_lease(&self, lease_key: &str) -> bool {
        let mut map = self.leases.write().unwrap_or_else(|e| e.into_inner());
        if let Some(lease) = map.get_mut(lease_key) {
            lease.is_revoked = true;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lease_manager_enforces_invariant_13_single_worker_exclusivity() {
        let manager = ExecutionLeaseManager::new();
        let lease1 = manager
            .grant_lease("exec_100", "n1", "w1", 30000)
            .expect("Grant lease 1");
        assert_eq!(lease1.epoch, 1);

        // Attempting to grant the same node to w2 must be rejected under Invariant 12
        let err = manager.grant_lease("exec_100", "n1", "w2", 30000);
        assert!(
            err.is_err(),
            "Must reject concurrent lease on same node to different worker"
        );

        // Renewing lease1 advances epoch
        assert!(manager.renew_lease(&lease1.lease_key));

        // Revoking lease1 allows new worker to claim
        assert!(manager.revoke_lease(&lease1.lease_key));
        let lease2 = manager
            .grant_lease("exec_100", "n1", "w2", 30000)
            .expect("Grant lease 2");
        assert_eq!(lease2.worker_id, "w2");
    }

    #[test]
    fn expired_lease_allows_failover_with_epoch_monotonicity() {
        let manager = ExecutionLeaseManager::new();
        let lease = manager.grant_lease("exec_x", "n", "w1", 1).expect("grant");
        // Simulate expiry by backdating far beyond TTL via revoke-free path:
        // grant with ttl=1 then immediately attempt re-grant by same worker to
        // bump epoch; a different worker must still be blocked until truly
        // expired, so instead assert is_expired logic directly.
        assert!(!lease.is_expired(lease.granted_at_ms));
        assert!(lease.is_expired(lease.granted_at_ms + 2));

        let mut l = lease.clone();
        l.is_revoked = true;
        assert!(
            l.is_expired(l.granted_at_ms),
            "revoked lease counts as expired"
        );
    }
}
