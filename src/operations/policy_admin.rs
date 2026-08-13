use std::sync::Arc;
use parking_lot::Mutex;
use crate::policy::PolicyDeclaration;
use crate::telemetry::audit::{AuditLog, AuditEntry};
use crate::operations::OperationError;

pub struct PolicyAdmin {
    store: Arc<Mutex<Vec<PolicyDeclaration>>>,
    audit_log: Arc<AuditLog>,
    registry: Option<Arc<crate::policy::PolicyRegistry>>,
}

impl PolicyAdmin {
    pub fn new(
        store: Arc<Mutex<Vec<PolicyDeclaration>>>,
        audit_log: Arc<AuditLog>,
    ) -> Self {
        Self { store, audit_log, registry: None }
    }

    pub fn new_with_registry(
        registry: Arc<crate::policy::PolicyRegistry>,
        store: Arc<Mutex<Vec<PolicyDeclaration>>>,
        audit_log: Arc<AuditLog>,
    ) -> Self {
        Self { store, audit_log, registry: Some(registry) }
    }

    pub fn list_policies(&self) -> Result<Vec<PolicyDeclaration>, OperationError> {
        let store = self.store.lock();
        Ok(store.clone())
    }

    #[allow(dead_code)]
    pub fn get_policy(&self, name: &str) -> Result<Option<PolicyDeclaration>, OperationError> {
        let store = self.store.lock();
        Ok(store.iter().find(|d| d.name == name).cloned())
    }

    pub fn create_policy(&self, decl: PolicyDeclaration) -> Result<(), OperationError> {
        let mut store = self.store.lock();
        if store.iter().any(|d| d.name == decl.name) {
            return Err(OperationError::Policy(format!("Policy '{}' already exists", decl.name)));
        }
        store.push(decl.clone());
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

    #[allow(dead_code)]
    pub fn update_policy(&self, name: &str, decl: PolicyDeclaration) -> Result<(), OperationError> {
        let mut store = self.store.lock();
        if let Some(existing) = store.iter_mut().find(|d| d.name == name) {
            *existing = decl.clone();
            self.audit_log.record(AuditEntry {
                timestamp: chrono::Utc::now().timestamp(),
                request_id: String::new(),
                user_id: None,
                action: format!("policy.update:{}", name),
                result: "ok".into(),
                details: serde_json::json!(decl),
            });
            Ok(())
        } else {
            Err(OperationError::Policy(format!("Policy '{}' not found", name)))
        }
    }

    pub fn delete_policy(&self, name: &str) -> Result<(), OperationError> {
        let mut store = self.store.lock();
        let len_before = store.len();
        store.retain(|d| d.name != name);
        if store.len() == len_before {
            return Err(OperationError::Policy(format!("Policy '{}' not found", name)));
        }
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

    #[test]
    fn test_create_and_list_policies() {
        let store = Arc::new(Mutex::new(Vec::new()));
        let audit = Arc::new(AuditLog::new(100));
        let admin = PolicyAdmin::new(store.clone(), audit.clone());

        let decl = PolicyDeclaration {
            name: "test-policy".into(),
            priority: 10,
            match_target: "shell.exec".into(),
            effect: "deny".into(),
            conditions: HashMap::new(),
            annotations: HashMap::new(),
        };
        admin.create_policy(decl.clone()).unwrap();
        let list = admin.list_policies().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-policy");
    }

    #[test]
    fn test_delete_policy() {
        let store = Arc::new(Mutex::new(Vec::new()));
        let audit = Arc::new(AuditLog::new(100));
        let admin = PolicyAdmin::new(store.clone(), audit.clone());

        let decl = PolicyDeclaration {
            name: "to-delete".into(),
            priority: 5,
            match_target: "http.*".into(),
            effect: "allow".into(),
            conditions: HashMap::new(),
            annotations: HashMap::new(),
        };
        admin.create_policy(decl).unwrap();
        admin.delete_policy("to-delete").unwrap();
        let list = admin.list_policies().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_create_duplicate_returns_error() {
        let store = Arc::new(Mutex::new(Vec::new()));
        let audit = Arc::new(AuditLog::new(100));
        let admin = PolicyAdmin::new(store.clone(), audit.clone());

        let decl = PolicyDeclaration {
            name: "dup".into(),
            priority: 1,
            match_target: "*".into(),
            effect: "allow".into(),
            conditions: HashMap::new(),
            annotations: HashMap::new(),
        };
        admin.create_policy(decl.clone()).unwrap();
        let result = admin.create_policy(decl);
        assert!(result.is_err());
    }
}
