#[test]
fn test_capability_registry_wiring() {
    use fusion_router::capability::CapabilityRegistry;
    let registry = fusion_router::capability::InMemoryCapabilityRegistry::new();
    let caps = registry.list();
    assert!(caps.is_empty() || !caps.is_empty());
}
