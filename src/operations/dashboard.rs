use std::sync::Arc;
use parking_lot::RwLock;
use crate::capability::{InMemoryCapabilityRegistry, CapabilityRegistry};
use crate::operations::{OperationError, RegistrySummary, RuntimeSummary, InvocationMetric, TimeWindow, RuntimeModuleCache};

pub trait DashboardDataProvider: Send + Sync {
    fn registry_summary(&self) -> Result<RegistrySummary, OperationError>;
    fn runtime_summary(&self) -> Result<RuntimeSummary, OperationError>;
    fn invocation_metrics(&self, window: TimeWindow) -> Result<Vec<InvocationMetric>, OperationError>;
}

pub struct DefaultDashboardDataProvider {
    registry: Arc<RwLock<InMemoryCapabilityRegistry>>,
    module_cache: Arc<RuntimeModuleCache>,
}

impl DefaultDashboardDataProvider {
    pub fn new(
        registry: Arc<RwLock<InMemoryCapabilityRegistry>>,
        module_cache: Arc<RuntimeModuleCache>,
    ) -> Self {
        Self { registry, module_cache }
    }
}

impl DashboardDataProvider for DefaultDashboardDataProvider {
    fn registry_summary(&self) -> Result<RegistrySummary, OperationError> {
        let reg = self.registry.read();
        let contracts = reg.list();
        let mut by_source = std::collections::HashMap::new();
        by_source.insert("builtin".into(), 0);
        by_source.insert("package".into(), 0);
        by_source.insert("development".into(), 0);
        by_source.insert("remote".into(), 0);
        let total = contracts.len();
        by_source.insert("builtin".into(), total);
        Ok(RegistrySummary {
            total_capabilities: total,
            by_source,
            frozen: reg.is_frozen(),
        })
    }

    fn runtime_summary(&self) -> Result<RuntimeSummary, OperationError> {
        let loaded = self.module_cache.len();
        Ok(RuntimeSummary {
            loaded_instances: loaded,
            total_memory_bytes: 0,
            total_fuel_consumed: 0,
            active_sessions: 0,
        })
    }

    fn invocation_metrics(&self, _window: TimeWindow) -> Result<Vec<InvocationMetric>, OperationError> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::InMemoryCapabilityRegistry;
    use crate::operations::RuntimeModuleCache;
    use fusion_plugin_api::{CapabilityContract, CapabilityId};

    fn populated_registry() -> InMemoryCapabilityRegistry {
        let mut reg = InMemoryCapabilityRegistry::new();
        let c1 = CapabilityContract {
            id: CapabilityId::new("alpha"),
            version: semver::Version::parse("1.0.0").unwrap(),
            description: "alpha cap".into(),
            inputs_schema: serde_json::json!({}),
            outputs_schema: serde_json::json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 0,
            reliability_score: 1.0,
            supports_streaming: false,
        };
        reg.register(c1).unwrap();
        reg.freeze();
        reg
    }

    #[test]
    fn test_registry_summary_returns_data() {
        let registry = Arc::new(RwLock::new(populated_registry()));
        let provider = DefaultDashboardDataProvider::new(
            registry.clone(),
            Arc::new(RuntimeModuleCache::new()),
        );
        let summary = provider.registry_summary().unwrap();
        assert_eq!(summary.total_capabilities, 1);
        assert!(summary.frozen);
    }

    #[test]
    fn test_runtime_summary_returns_zero_when_empty() {
        let registry = Arc::new(RwLock::new(InMemoryCapabilityRegistry::new()));
        let provider = DefaultDashboardDataProvider::new(
            registry.clone(),
            Arc::new(RuntimeModuleCache::new()),
        );
        let summary = provider.runtime_summary().unwrap();
        assert_eq!(summary.loaded_instances, 0);
    }
}
