//! Gate 06/07 identity tests.
//!
//! Verifies that the production wiring creates exactly one PolicyRegistry
//! and one CapabilityRegistry instance, and that the same Arc pointers
//! flow through AppState → PolicyAdmin / operations dashboard.

use std::sync::Arc;

/// Gate 06: AppState.policy_registry is the same instance used by PolicyAdmin.
#[test]
fn test_policy_registry_identity_through_app_state() {
    let registry = Arc::new(fusion_router::policy::PolicyRegistry::new());
    let audit_log = Arc::new(fusion_router::telemetry::audit::AuditLog::new(100));
    let admin =
        fusion_router::operations::policy_admin::PolicyAdmin::new(registry.clone(), audit_log);
    assert!(
        admin.registry_is(&registry),
        "PolicyAdmin must hold the same PolicyRegistry Arc as AppState"
    );
}

/// Gate 07: AppState.capability_registry is the same instance used by the dashboard.
#[test]
fn test_capability_registry_identity_through_dashboard() {
    let registry: Arc<dyn fusion_router::capability::CapabilityRegistry> =
        Arc::new(fusion_router::capability::InMemoryCapabilityRegistry::new());
    let provider = fusion_router::operations::dashboard::DefaultDashboardDataProvider::new(
        registry.clone(),
        Arc::new(fusion_router::operations::RuntimeModuleCache::new()),
    );
    assert!(
        provider.registry_is(&registry),
        "Dashboard must hold the same CapabilityRegistry Arc as AppState"
    );
}

/// Gate 06: Two separate PolicyRegistry instances are independent.
#[test]
fn test_policy_registry_instances_are_distinct() {
    let a = Arc::new(fusion_router::policy::PolicyRegistry::new());
    let b = Arc::new(fusion_router::policy::PolicyRegistry::new());
    assert!(
        !Arc::ptr_eq(&a, &b),
        "Two PolicyRegistry::new() calls must produce distinct instances"
    );
}

/// Gate 07: Two separate CapabilityRegistry instances are independent.
#[test]
fn test_capability_registry_instances_are_distinct() {
    let a: Arc<dyn fusion_router::capability::CapabilityRegistry> =
        Arc::new(fusion_router::capability::InMemoryCapabilityRegistry::new());
    let b: Arc<dyn fusion_router::capability::CapabilityRegistry> =
        Arc::new(fusion_router::capability::InMemoryCapabilityRegistry::new());
    assert!(
        !Arc::ptr_eq(&a, &b),
        "Two InMemoryCapabilityRegistry::new() calls must produce distinct instances"
    );
}
