//! Capability Subsystem (`src/capability/mod.rs`)
//!
//! Provides the immutable runtime `CapabilityRegistry` and contract abstractions.

use std::collections::HashMap;
use std::sync::Arc;
use fusion_plugin_api::{CapabilityContract, CapabilityId, CapabilityInstance};

/// Thread-safe, immutable registry of all capabilities active after plugin discovery freeze.
#[derive(Debug, Clone)]
pub struct CapabilityRegistry {
    contracts: HashMap<CapabilityId, CapabilityContract>,
    frozen: bool,
}

impl CapabilityRegistry {
    /// Constructs a new mutable registry for startup population.
    pub fn new() -> Self {
        Self {
            contracts: HashMap::new(),
            frozen: false,
        }
    }

    /// Registers a `CapabilityContract` before the registry is frozen.
    pub fn register(&mut self, contract: CapabilityContract) -> Result<(), String> {
        if self.frozen {
            return Err("Cannot register capability: CapabilityRegistry is frozen".into());
        }
        self.contracts.insert(contract.id.clone(), contract);
        Ok(())
    }

    /// Freezes the registry, returning an immutable thread-safe `Arc<CapabilityRegistry>`.
    pub fn freeze(mut self) -> Arc<Self> {
        self.frozen = true;
        Arc::new(self)
    }

    /// Queries a `CapabilityContract` by `CapabilityId`.
    pub fn get(&self, id: &CapabilityId) -> Option<&CapabilityContract> {
        self.contracts.get(id)
    }

    /// Checks if a capability exists in the registry.
    pub fn contains(&self, id: &CapabilityId) -> bool {
        self.contracts.contains_key(id)
    }

    /// Returns an iterator over all registered capability contracts.
    pub fn list(&self) -> Vec<&CapabilityContract> {
        self.contracts.values().collect()
    }

    /// Instantiates a bound `CapabilityInstance` with runtime execution parameters.
    pub fn instantiate(
        &self,
        id: &CapabilityId,
        runtime_params: serde_json::Value,
    ) -> Result<CapabilityInstance, String> {
        let contract = self
            .get(id)
            .ok_or_else(|| format!("Capability not found: {}", id))?;

        Ok(CapabilityInstance {
            contract: contract.clone(),
            runtime_params,
        })
    }

    /// Indicates whether the registry has been frozen.
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_capability_registry_freeze() {
        let mut reg = CapabilityRegistry::new();
        let contract = CapabilityContract {
            id: CapabilityId::new("test.echo"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Test contract".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 1,
            reliability_score: 1.0,
            supports_streaming: false,
        };

        reg.register(contract.clone()).unwrap();
        assert!(!reg.is_frozen());

        let frozen_reg = reg.freeze();
        assert!(frozen_reg.is_frozen());
        assert!(frozen_reg.contains(&CapabilityId::new("test.echo")));
    }
}
