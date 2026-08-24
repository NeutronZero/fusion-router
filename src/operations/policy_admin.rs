use crate::operations::OperationError;
use crate::policy::PolicyDeclaration;
use crate::policy::PolicyRegistry;
use crate::telemetry::audit::{AuditEntry, AuditLog};
use fusion_planner::PolicySnapshot;
use std::sync::Arc;

const VALID_EFFECTS: [&str; 3] = ["deny", "approval", "allow"];

/// Fail-closed validation at the administrative boundary.
fn validate_declaration(decl: &PolicyDeclaration) -> Result<(), OperationError> {
    if decl.name.trim().is_empty() {
        return Err(OperationError::Policy(
            "policy name must not be empty".into(),
        ));
    }
    if decl.match_target.trim().is_empty() {
        return Err(OperationError::Policy(
            "policy match_target must not be empty".into(),
        ));
    }
    if !VALID_EFFECTS.contains(&decl.effect.as_str()) {
        return Err(OperationError::Policy(format!(
            "invalid effect '{}' (expected one of: deny, approval, allow)",
            decl.effect
        )));
    }
    Ok(())
}

fn parse_declaration(
    name: &str,
    id: &str,
    rule_json: &str,
) -> Result<PolicyDeclaration, OperationError> {
    serde_json::from_str(rule_json).map_err(|e| {
        OperationError::Policy(format!("stored policy '{name}' (id {id}) is corrupt: {e}"))
    })
}

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
        Self {
            registry,
            audit_log,
        }
    }

    /// Returns `true` if `self` holds the same `PolicyRegistry` instance as `other`.
    /// Used by Gate 06 identity tests to verify single-instance wiring.
    pub fn registry_is(&self, other: &Arc<PolicyRegistry>) -> bool {
        Arc::ptr_eq(&self.registry, other)
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
    ///
    /// Entries are deserialized from their stored declarations — the listed
    /// effects/priorities/conditions are exactly what was created.
    pub fn list_policies(&self) -> Result<Vec<PolicyDeclaration>, OperationError> {
        let snap = self.registry.current_snapshot();
        snap.policies
            .iter()
            .map(|p| parse_declaration(p.name.as_str(), p.id.as_str(), &p.rule))
            .collect()
    }

    /// Gets a policy by name.
    pub fn get_policy(&self, name: &str) -> Result<Option<PolicyDeclaration>, OperationError> {
        let snap = self.registry.current_snapshot();
        match snap.policies.iter().find(|p| p.name == name) {
            Some(p) => Ok(Some(parse_declaration(&p.name, &p.id, &p.rule)?)),
            None => Ok(None),
        }
    }

    /// Creates a new policy. Rejects duplicates by name and invalid effects.
    pub fn create_policy(&self, decl: PolicyDeclaration) -> Result<(), OperationError> {
        validate_declaration(&decl)?;
        if self.registry.has_policy(&decl.name) {
            return Err(OperationError::Policy(format!(
                "Policy '{}' already exists",
                decl.name
            )));
        }
        self.store(decl, "create")
    }

    /// Updates an existing policy by name. Rejects invalid effects.
    /// Policy names are immutable identifiers; delete and recreate to rename.
    pub fn update_policy(&self, name: &str, decl: PolicyDeclaration) -> Result<(), OperationError> {
        validate_declaration(&decl)?;
        if !self.registry.has_policy(name) {
            return Err(OperationError::Policy(format!(
                "Policy '{}' not found",
                name
            )));
        }
        if decl.name != name {
            return Err(OperationError::Policy(
                "policy name is immutable; delete and recreate to rename".into(),
            ));
        }
        self.store(decl, "update").map(|_| ())
    }

    fn store(&self, decl: PolicyDeclaration, action: &str) -> Result<(), OperationError> {
        let rule_json = serde_json::to_string(&decl)
            .map_err(|e| OperationError::Policy(format!("failed to encode policy: {e}")))?;
        let audited_name = decl.name.clone();
        let audited_decl = serde_json::json!(decl);
        self.registry
            .apply_policy(audited_name.clone(), audited_name.clone(), rule_json);
        self.audit_log.record(AuditEntry {
            timestamp: chrono::Utc::now().timestamp(),
            request_id: String::new(),
            user_id: None,
            action: format!("policy.{action}:{audited_name}"),
            result: "ok".into(),
            details: audited_decl,
        });
        Ok(())
    }

    /// Deletes a policy by name.
    pub fn delete_policy(&self, name: &str) -> Result<(), OperationError> {
        if !self.registry.has_policy(name) {
            return Err(OperationError::Policy(format!(
                "Policy '{}' not found",
                name
            )));
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
        admin
            .update_policy("original", test_decl("original"))
            .unwrap();
        let snap = admin.current_snapshot();
        assert_eq!(snap.policies.len(), 1, "update must replace, not duplicate");
        assert_eq!(snap.policies[0].name, "original");
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

    #[test]
    fn created_deny_policy_round_trips_with_real_effect() {
        let admin = test_admin();
        admin.create_policy(test_decl("deny-shell")).unwrap();
        let listed = admin.list_policies().unwrap();
        assert_eq!(listed[0].effect, "deny", "effect must survive storage");
        assert_eq!(listed[0].match_target, "shell.exec");
        assert_eq!(
            admin.get_policy("deny-shell").unwrap().unwrap().effect,
            "deny"
        );
    }

    #[test]
    fn create_rejects_invalid_effect_fail_closed() {
        let admin = test_admin();
        let mut decl = test_decl("bad");
        decl.effect = "Deny".into();
        assert!(
            admin.create_policy(decl).is_err(),
            "case mismatch must be rejected"
        );
        let mut decl2 = test_decl("bad2");
        decl2.effect = "block".into();
        assert!(admin.create_policy(decl2).is_err());
        assert_eq!(admin.current_snapshot().policies.len(), 0, "nothing stored");
    }

    #[test]
    fn update_stores_new_declaration_content() {
        let admin = test_admin();
        admin.create_policy(test_decl("p")).unwrap();
        let mut updated = test_decl("p");
        updated.effect = "approval".into();
        updated.priority = 42;
        admin.update_policy("p", updated).unwrap();
        let got = admin.get_policy("p").unwrap().unwrap();
        assert_eq!(got.effect, "approval");
        assert_eq!(got.priority, 42);
    }

    #[test]
    fn registry_policy_ir_sees_admin_created_deny() {
        let admin = test_admin();
        admin.create_policy(test_decl("deny-shell")).unwrap();
        // The registry is the same instance the chat path reads from.
        let ir = admin.registry.policy_ir().unwrap().expect("IR present");
        assert_eq!(ir.rules[0].effect, crate::policy::ir::PolicyEffect::Deny);
    }
}
