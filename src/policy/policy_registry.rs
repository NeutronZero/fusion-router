use fusion_planner::{PolicyDeclarationSnapshot, PolicySnapshot};
use parking_lot::RwLock;

/// Immutable triple read/written atomically: a version number and the exact
/// policy list that version contains, plus the append-only history. Holding
/// them in one lock guarantees readers never observe an old version paired
/// with new rules (or vice versa).
struct PolicyState {
    version: u64,
    policies: Vec<PolicyDeclarationSnapshot>,
    history: Vec<PolicySnapshot>,
}

/// Authoritative policy registry emitting versioned immutable snapshots.
///
/// All policy mutations go through this registry. Each mutation increments the
/// version counter and appends an immutable `PolicySnapshot` to the history.
/// The current snapshot is always available via `current_snapshot()`, and
/// historical snapshots can be retrieved via `snapshot_at(version)`.
pub struct PolicyRegistry {
    state: RwLock<PolicyState>,
}

impl Default for PolicyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyRegistry {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(Self::initial_state()),
        }
    }

    fn initial_state() -> PolicyState {
        let initial = PolicySnapshot {
            version: 1,
            policies: vec![],
            created_at: now_epoch_secs(),
        };
        PolicyState {
            version: 1,
            policies: vec![],
            history: vec![initial],
        }
    }

    /// Fixed empty registry for OFFLINE verification contexts only (release
    /// gates, replay harness, examples). Production serving must share the
    /// AppState singleton (Invariant 17 / Convergence Gate 06); never wire
    /// this into a live server path. Restricted to `pub(crate)` so external
    /// callers cannot accidentally use an offline registry as the live
    /// singleton — use `PolicyRegistry :: new` via `AppState` instead.
    pub(crate) fn offline_default() -> Self {
        Self::new()
    }

    /// Returns the current (latest) policy snapshot.
    ///
    /// Version and policies are captured under one lock, so the returned
    /// snapshot is always a consistent (version, rules) pair.
    pub fn current_snapshot(&self) -> PolicySnapshot {
        let state = self.state.read();
        PolicySnapshot {
            version: state.version,
            policies: state.policies.clone(),
            created_at: now_epoch_secs(),
        }
    }

    /// Returns a specific historical snapshot by version number.
    /// Returns `None` if the version does not exist.
    pub fn snapshot_at(&self, version: u64) -> Option<PolicySnapshot> {
        self.state
            .read()
            .history
            .iter()
            .find(|s| s.version == version)
            .cloned()
    }

    /// Returns all historical snapshots (append-only log).
    pub fn snapshot_history(&self) -> Vec<PolicySnapshot> {
        self.state.read().history.clone()
    }

    /// Returns the number of mutations applied (version - 1).
    pub fn mutation_count(&self) -> u64 {
        self.state.read().version.saturating_sub(1)
    }

    /// Applies a policy declaration (upsert by id), increments version,
    /// and appends the new snapshot to history.
    pub fn apply_policy(&self, id: String, name: String, rule: String) -> PolicySnapshot {
        let mut state = self.state.write();
        state.policies.retain(|p| p.id != id);
        state
            .policies
            .push(PolicyDeclarationSnapshot { id, name, rule });
        state.version += 1;
        let snapshot = PolicySnapshot {
            version: state.version,
            policies: state.policies.clone(),
            created_at: now_epoch_secs(),
        };
        state.history.push(snapshot.clone());
        snapshot
    }

    /// Removes a policy declaration by id, increments version,
    /// and appends the new snapshot to history.
    pub fn remove_policy(&self, id: &str) -> PolicySnapshot {
        let mut state = self.state.write();
        state.policies.retain(|p| p.id != id);
        state.version += 1;
        let snapshot = PolicySnapshot {
            version: state.version,
            policies: state.policies.clone(),
            created_at: now_epoch_secs(),
        };
        state.history.push(snapshot.clone());
        snapshot
    }

    /// Returns the number of active policies.
    pub fn policy_count(&self) -> usize {
        self.state.read().policies.len()
    }

    /// Builds the compiler-facing `PolicyIR` from the current snapshot.
    ///
    /// `Ok(None)` when no policies are active (no policy pass is appended).
    /// Fail-closed: a malformed stored rule is an error, not a skip.
    pub fn policy_ir(&self) -> Result<Option<crate::policy::ir::PolicyIR>, String> {
        let snap = self.current_snapshot();
        if snap.policies.is_empty() {
            return Ok(None);
        }
        crate::policy::ir::PolicyIR::from_policy_snapshot(&snap).map(Some)
    }

    /// Checks if a policy with the given id exists.
    pub fn has_policy(&self, id: &str) -> bool {
        self.state.read().policies.iter().any(|p| p.id == id)
    }
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_registry_starts_at_version_1_with_empty_history() {
        let reg = PolicyRegistry::new();
        assert_eq!(reg.current_snapshot().version, 1);
        assert!(reg.current_snapshot().policies.is_empty());
        assert_eq!(reg.snapshot_history().len(), 1);
        assert_eq!(reg.mutation_count(), 0);
    }

    #[test]
    fn apply_policy_increments_version_and_records_history() {
        let reg = PolicyRegistry::new();
        let snap = reg.apply_policy("p1".into(), "deny-shell".into(), "deny shell.exec".into());
        assert_eq!(snap.version, 2);
        assert_eq!(snap.policies.len(), 1);
        assert_eq!(reg.mutation_count(), 1);
        assert_eq!(reg.snapshot_history().len(), 2);
    }

    #[test]
    fn remove_policy_decrements_count_and_records_history() {
        let reg = PolicyRegistry::new();
        reg.apply_policy("p1".into(), "test".into(), "rule".into());
        let snap = reg.remove_policy("p1");
        assert_eq!(snap.version, 3);
        assert!(snap.policies.is_empty());
        assert_eq!(reg.snapshot_history().len(), 3);
    }

    #[test]
    fn snapshot_at_returns_historical_version() {
        let reg = PolicyRegistry::new();
        reg.apply_policy("p1".into(), "test".into(), "rule".into());
        let v1 = reg.snapshot_at(1).expect("v1 should exist");
        assert_eq!(v1.version, 1);
        assert!(v1.policies.is_empty());
        let v2 = reg.snapshot_at(2).expect("v2 should exist");
        assert_eq!(v2.version, 2);
        assert_eq!(v2.policies.len(), 1);
    }

    #[test]
    fn snapshot_at_returns_none_for_missing_version() {
        let reg = PolicyRegistry::new();
        assert!(reg.snapshot_at(99).is_none());
    }

    #[test]
    fn apply_policy_upserts_by_id() {
        let reg = PolicyRegistry::new();
        reg.apply_policy("p1".into(), "first".into(), "rule1".into());
        reg.apply_policy("p1".into(), "updated".into(), "rule2".into());
        assert_eq!(reg.policy_count(), 1);
        let snap = reg.current_snapshot();
        assert_eq!(snap.policies[0].name, "updated");
        assert_eq!(snap.policies[0].rule, "rule2");
    }

    #[test]
    fn has_policy_checks_existence() {
        let reg = PolicyRegistry::new();
        assert!(!reg.has_policy("p1"));
        reg.apply_policy("p1".into(), "test".into(), "rule".into());
        assert!(reg.has_policy("p1"));
    }

    #[test]
    fn policy_ir_is_none_when_no_policies() {
        let reg = PolicyRegistry::new();
        assert!(reg.policy_ir().unwrap().is_none());
    }

    #[test]
    fn policy_ir_carries_deny_from_stored_declaration() {
        let reg = PolicyRegistry::new();
        let decl = crate::policy::ast::PolicyDeclaration {
            name: "deny-shell".into(),
            priority: 7,
            match_target: "shell.exec".into(),
            effect: "deny".into(),
            conditions: Default::default(),
            annotations: Default::default(),
        };
        reg.apply_policy(
            "deny-shell".into(),
            "deny-shell".into(),
            serde_json::to_string(&decl).unwrap(),
        );
        let ir = reg.policy_ir().unwrap().expect("IR for non-empty registry");
        assert_eq!(ir.rules.len(), 1);
        assert_eq!(ir.rules[0].effect, crate::policy::ir::PolicyEffect::Deny);
        assert_eq!(ir.rules[0].target_pattern, "shell.exec");
    }

    #[test]
    fn policy_ir_fails_closed_on_corrupt_rule() {
        let reg = PolicyRegistry::new();
        reg.apply_policy("x".into(), "x".into(), "garbage".into());
        assert!(reg.policy_ir().is_err(), "corrupt rules must fail closed");
    }

    /// Invariant under concurrent mutation: a given version must always map
    /// to the SAME rule set. With the historical split-lock implementation a
    /// reader could observe an old version paired with new rules (torn
    /// snapshot); this stress test fails on any such tear.
    #[test]
    fn stress_concurrent_readers_never_observe_torn_snapshots() {
        use std::collections::HashMap;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex};

        let registry = Arc::new(PolicyRegistry::new());
        let stop = Arc::new(AtomicBool::new(false));

        // (version -> policies.len()) observed by readers; the same version
        // must never be seen with two different lengths.
        let observations: Arc<Mutex<HashMap<u64, usize>>> = Arc::new(Mutex::new(HashMap::new()));

        let mut readers = Vec::new();
        for _ in 0..6 {
            let registry = Arc::clone(&registry);
            let stop = Arc::clone(&stop);
            let observations = Arc::clone(&observations);
            readers.push(std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let snap = registry.current_snapshot();
                    let mut obs = observations.lock().unwrap();
                    match obs.get(&snap.version) {
                        Some(seen_len) => {
                            assert_eq!(
                                *seen_len,
                                snap.policies.len(),
                                "TORN SNAPSHOT: version {} observed with both {} and {} rules",
                                snap.version,
                                seen_len,
                                snap.policies.len()
                            );
                        }
                        None => {
                            obs.insert(snap.version, snap.policies.len());
                        }
                    }
                }
            }));
        }

        const WRITERS: usize = 4;
        const OPS_PER_WRITER: usize = 250;
        let mut writers = Vec::new();
        for w in 0..WRITERS {
            let registry = Arc::clone(&registry);
            writers.push(std::thread::spawn(move || {
                for i in 0..OPS_PER_WRITER {
                    registry.apply_policy(
                        format!("p{w}-{i}"),
                        "stress".into(),
                        format!("rule-{w}-{i}"),
                    );
                }
            }));
        }
        for writer in writers {
            writer.join().unwrap();
        }

        stop.store(true, Ordering::Relaxed);
        for reader in readers {
            reader.join().unwrap();
        }

        // Post-conditions: versioning is contiguous and every history entry
        // agrees with what readers recorded for that version.
        assert_eq!(
            registry.mutation_count(),
            (WRITERS * OPS_PER_WRITER) as u64,
            "every mutation must have landed exactly once"
        );
        let final_version = registry.current_snapshot().version;
        assert_eq!(final_version, 1 + (WRITERS * OPS_PER_WRITER) as u64);

        let history = registry.snapshot_history();
        assert_eq!(history.len(), final_version as usize);
        for window in history.windows(2) {
            assert_eq!(
                window[1].version,
                window[0].version + 1,
                "contiguous versions"
            );
        }

        let observations = observations.lock().unwrap();
        for snapshot in &history {
            if let Some(seen_len) = observations.get(&snapshot.version) {
                assert_eq!(
                    *seen_len,
                    snapshot.policies.len(),
                    "reader view of version {} diverged from authoritative history",
                    snapshot.version
                );
            }
        }
    }
}
