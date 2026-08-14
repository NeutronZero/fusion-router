use std::sync::Arc;
use crate::policy::PolicyDeclaration;
use crate::policy::PolicyRegistry;
use crate::telemetry::audit::{AuditLog, AuditEntry};
use crate::operations::OperationError;
use fusion_planner::PolicySnapshot;

/// Thin administrative facade over the authoritative `PolicyRegistry`.
///
/// All CRUD operations delegate to the registry, which maintains versioned
/// immutable snapshots. The audit log records every mutation for traceability.
pub struct PolicyAdmin {
    registry: Arc<PolicyRegistry>,
    audit_log: Arc<AuditLog>,
}

impl PolicyAdmin {
    pub fn new(registry: Arc<PolicyRegistry>, audit_log: Arc<AuditLog>) -> Self {
        Self { registry, audit_log }
    }

    /// Returns the current policy snapshot from the registry.
    pub fn current_snapshot(&self) -> PolicySnapshot {
        self.registry.current_snapshot()
    }

    /// Returns a specific historical snapshot by version.
    pub fn snapshot_at(&self, version: u64) -> Option<PolicySnapshot> {
        self.registry.snapshot_at(version)
    }

    /// Returns all historical snapshots.
    pub fn snapshot_history(&self) -> Vec<PolicySnapshot> {
        self.registry.snapshot_history()
    }

    /// Lists all active policies as `PolicyDeclaration` values.
    pub fn list_policies(&self) -> Result<Vec<PolicyDeclaration>, OperationError> {
        let snap = self.registry.current_snapshot();
        Ok(snap.policies.into_iter().map(|p| PolicyDeclaration {
            name: p.name,
            priority: 0,
            match_target: p.rule.clone(),
            effect: "allow".into(),
            conditions: Default::default(),
            annotations: Default::default(),
        }).collect())
    }

    /// Gets a policy by name.
    pub fn get_policy(&self, name: &str) -> Result<Option<PolicyDeclaration>, OperationError> {
        let snap = self.registry.current_snapshot();
        Ok(snap.policies.iter().find(|p| p.name == name).map(|p| PolicyDeclaration {
            name: p.name.clone(),
            priority: 0,
            match_target: p.rule.clone(),
            effect: "allow".into(),
            conditions: Default::default(),
            annotations: Default::default(),
        }))
    }

    /// Creates a new policy. Rejects duplicates by name.
    pub fn create_policy(&self, decl: PolicyDeclaration) -> Result<(), OperationError> {
        if self.registry.has_policy(&decl.name) {
            return Err(OperationError::Policy(format!("Policy '{}' already exists", decl.name)));
        }
        self.registry.apply_policy(
            decl.name.clone(),
            decl.name.clone(),
            decl.match_target.clone(),
        );
        self.audit_log.record(AuditEntry {
            timestamp: chrono::Utc::now().timestamp(),
            request_id: String::new(),
            user_id: None,
            action: format!("policy.create:{}", decl.name),
            result: "ok".into(),
            details: serde_json::json!(decl),
        });
        Ok(())
    }

    /// Updates an existing policy by name.
    pub fn update_policy(&self, name: &str, decl: PolicyDeclaration) -> Result<(), OperationError> {
        if !self.registry.has_policy(name) {
            return Err(OperationError::Policy(format!("Policy '{}' not found", name)));
        }
        self.registry.apply_policy(
            name.to_string(),
            decl.name.clone(),
            decl.match_target.clone(),
        );
        self.audit_log.record(AuditEntry {
            timestamp: chrono::Utc::now().timestamp(),
            request_id: String::new(),
            user_id: None,
            action: format!("policy.update:{}", name),
            result: "ok".into(),
            details: serde_json::json!(decl),
        });
        Ok(())
    }

    /// Deletes a policy by name.
    pub fn delete_policy(&self, name: &str) -> Result<(), OperationError> {
        if !self.registry.has_policy(name) {
            return Err(OperationError::Policy(format!("Policy '{}' not found", name)));
        }
        self.registry.remove_policy(name);
        self.audit_log.record(AuditEntry {
            timestamp: chrono::Utc::now().timestamp(),
            request_id: String::new(),
            user_id: None,
            action: format!("policy.delete:{}", name),
            result: "ok".into(),
            details: serde_json::json!({"name": name}),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::PolicyDeclaration;
    use crate::telemetry::audit::AuditLog;
    use std::collections::HashMap;

    fn test_admin() -> PolicyAdmin {
        let registry = Arc::new(PolicyRegistry::new());
        let audit = Arc::new(AuditLog::new(100));
        PolicyAdmin::new(registry, audit)
    }

    fn test_decl(name: &str) -> PolicyDeclaration {
        PolicyDeclaration {
            name: name.into(),
            priority: 10,
            match_target: "shell.exec".into(),
            effect: "deny".into(),
            conditions: HashMap::new(),
            annotations: HashMap::new(),
        }
    }

    #[test]
    fn test_admin_keeps_authoritative_registry_instance() {
        let registry = Arc::new(PolicyRegistry::new());
        let audit = Arc::new(AuditLog::new(100));
        let admin = PolicyAdmin::new(registry.clone(), audit);
        assert!(Arc::ptr_eq(&admin.registry, &registry));
    }

    #[test]
    fn test_create_and_list_policies() {
        let admin = test_admin();
        admin.create_policy(test_decl("test-policy")).unwrap();
        let list = admin.list_policies().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-policy");
    }

    #[test]
    fn test_delete_policy() {
        let admin = test_admin();
        admin.create_policy(test_decl("to-delete")).unwrap();
        admin.delete_policy("to-delete").unwrap();
        let list = admin.list_policies().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_create_duplicate_returns_error() {
        let admin = test_admin();
        admin.create_policy(test_decl("dup")).unwrap();
        let result = admin.create_policy(test_decl("dup"));
        assert!(result.is_err());
    }

    #[test]
    fn test_update_policy() {
        let admin = test_admin();
        admin.create_policy(test_decl("original")).unwrap();
        admin.update_policy("original", test_decl("updated")).unwrap();
        let snap = admin.current_snapshot();
        assert_eq!(snap.policies.len(), 1);
        assert_eq!(snap.policies[0].name, "updated");
    }

    #[test]
    fn test_delete_nonexistent_returns_error() {
        let admin = test_admin();
        assert!(admin.delete_policy("ghost").is_err());
    }

    #[test]
    fn test_snapshot_history_grows() {
        let admin = test_admin();
        assert_eq!(admin.snapshot_history().len(), 1);
        admin.create_policy(test_decl("p1")).unwrap();
        assert_eq!(admin.snapshot_history().len(), 2);
        admin.delete_policy("p1").unwrap();
        assert_eq!(admin.snapshot_history().len(), 3);
    }

    #[test]
    fn test_current_snapshot_reflects_state() {
        let admin = test_admin();
        let snap = admin.current_snapshot();
        assert_eq!(snap.version, 1);
        assert!(snap.policies.is_empty());
        admin.create_policy(test_decl("p1")).unwrap();
        let snap = admin.current_snapshot();
        assert_eq!(snap.version, 2);
        assert_eq!(snap.policies.len(), 1);
    }
}
