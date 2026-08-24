use fusion_router::compiler::registry::StrategyRegistry;
use fusion_router::strategies::consensus::ConsensusStrategy;
use fusion_router::strategies::single::SingleStrategy;
use std::sync::Arc;

#[test]
fn test_strategy_registry_compliance() {
    let mut registry = StrategyRegistry::new();
    registry.register(Arc::new(SingleStrategy));
    registry.register(Arc::new(ConsensusStrategy::default()));

    assert!(registry.contains("single"));
    assert!(registry.contains("consensus"));
    assert!(!registry.contains("unknown"));

    let single = registry.get("single");
    assert!(single.is_ok());
    assert_eq!(single.unwrap().descriptor().name, "Single");
}
