use fusion_planner::{PolicyDeclarationSnapshot, PolicySnapshot};
use parking_lot::RwLock;

pub struct PolicyRegistry {
    version: RwLock<u64>,
    policies: RwLock<Vec<PolicyDeclarationSnapshot>>,
}

impl Default for PolicyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyRegistry {
    pub fn new() -> Self {
        Self {
            version: RwLock::new(1),
            policies: RwLock::new(Vec::new()),
        }
    }

    pub fn current_snapshot(&self) -> PolicySnapshot {
        let ver = *self.version.read();
        let list = self.policies.read().clone();
        PolicySnapshot {
            version: ver,
            policies: list,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    pub fn apply_policy(&self, id: String, name: String, rule: String) -> PolicySnapshot {
        let mut list = self.policies.write();
        list.retain(|p| p.id != id);
        list.push(PolicyDeclarationSnapshot { id, name, rule });
        let mut ver = self.version.write();
        *ver += 1;
        PolicySnapshot {
            version: *ver,
            policies: list.clone(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}
