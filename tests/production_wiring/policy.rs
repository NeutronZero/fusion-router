#[test]
fn test_policy_registry_wiring() {
    let registry = fusion_router::policy::PolicyRegistry::new();
    let snap = registry.current_snapshot();
    assert_eq!(snap.version, 1);
    let updated = registry.apply_policy("p1".into(), "Policy 1".into(), "allow".into());
    assert_eq!(updated.version, 2);
    assert_eq!(updated.policies.len(), 1);
}
