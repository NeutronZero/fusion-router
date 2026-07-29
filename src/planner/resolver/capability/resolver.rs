//! Phase 2B & 2C — `CapabilityResolver`, `RequirementSet`, `ResolvedCapabilitySet`, & `CapabilityPlannerCache`
//!
//! Symbol resolution for capabilities, matching intent requirements to frozen contracts.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use parking_lot::Mutex;
use fusion_plugin_api::{CapabilityId, CapabilityInstance};
use crate::capability::CapabilityRegistry;
use super::graph::CapabilityGraph;

/// Represents extracted intent requirements requesting capability lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RequirementSet {
    pub required_capabilities: Vec<CapabilityId>,
    pub optional_capabilities: Vec<CapabilityId>,
    pub max_acceptable_latency_ms: Option<u64>,
    pub max_cost_usd: Option<u64>, // Scaled by 10^6 for integer hashing
}

impl RequirementSet {
    pub fn new(required: Vec<CapabilityId>) -> Self {
        Self {
            required_capabilities: required,
            optional_capabilities: Vec::new(),
            max_acceptable_latency_ms: None,
            max_cost_usd: None,
        }
    }
}

/// Output of symbol resolution containing resolved contracts and instantiated runtime handles.
#[derive(Debug, Clone)]
pub struct ResolvedCapabilitySet {
    pub graph: CapabilityGraph,
    pub instances: Vec<CapabilityInstance>,
}

/// LRU / In-memory planner cache mapping `RequirementSet` hash -> `ResolvedCapabilitySet`.
#[derive(Debug)]
pub struct CapabilityPlannerCache {
    cache: Mutex<HashMap<RequirementSet, ResolvedCapabilitySet>>,
    max_capacity: usize,
}

impl CapabilityPlannerCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            max_capacity: capacity,
        }
    }

    pub fn get(&self, reqs: &RequirementSet) -> Option<ResolvedCapabilitySet> {
        let guard = self.cache.lock();
        guard.get(reqs).cloned()
    }

    pub fn put(&self, reqs: RequirementSet, resolved: ResolvedCapabilitySet) {
        let mut guard = self.cache.lock();
        if guard.len() >= self.max_capacity {
            // Primitive eviction if at capacity
            if let Some(key) = guard.keys().next().cloned() {
                guard.remove(&key);
            }
        }
        guard.insert(reqs, resolved);
    }
}

/// Symbol resolver matching `RequirementSet` against an immutable `CapabilityRegistry`.
pub struct CapabilityResolver {
    registry: Arc<dyn CapabilityRegistry>,
    cache: CapabilityPlannerCache,
    aliases: HashMap<CapabilityId, CapabilityId>,
}

impl CapabilityResolver {
    pub fn new(registry: Arc<dyn CapabilityRegistry>) -> Self {
        Self {
            registry,
            cache: CapabilityPlannerCache::new(100),
            aliases: HashMap::new(),
        }
    }

    /// Registers a capability alias mapping (legacy capability -> active capability).
    pub fn register_alias(&mut self, alias: CapabilityId, target: CapabilityId) {
        self.aliases.insert(alias, target);
    }

    /// Resolves a `RequirementSet` into a `ResolvedCapabilitySet` with dependency graph checks.
    pub fn resolve(&self, reqs: &RequirementSet) -> Result<ResolvedCapabilitySet, String> {
        // 1. Check Planner Cache
        if let Some(cached) = self.cache.get(reqs) {
            return Ok(cached);
        }

        let mut graph = CapabilityGraph::new();
        let mut instances = Vec::new();
        let mut visited = HashSet::new();

        // 2. Resolve Required Capabilities
        for req_id in &reqs.required_capabilities {
            let target_id = self.aliases.get(req_id).unwrap_or(req_id);

            let contract = self.registry.get(target_id).ok_or_else(|| {
                format!("Capability resolution failed: required capability '{}' not registered", req_id)
            })?;

            // Check latency bound if specified
            if let Some(max_lat) = reqs.max_acceptable_latency_ms {
                if contract.estimated_latency_ms > max_lat {
                    return Err(format!(
                        "Capability '{}' latency ({}ms) exceeds requirement limit ({}ms)",
                        contract.id, contract.estimated_latency_ms, max_lat
                    ));
                }
            }

            if visited.insert(contract.id.clone()) {
                graph.add_node(contract.clone());
                instances.push(CapabilityInstance {
                    contract: contract.clone(),
                    runtime_params: serde_json::json!({}),
                });
            }
        }

        // 3. Resolve Optional Capabilities (Graceful Degradation)
        for opt_id in &reqs.optional_capabilities {
            let target_id = self.aliases.get(opt_id).unwrap_or(opt_id);
            if let Some(contract) = self.registry.get(target_id) {
                if visited.insert(contract.id.clone()) {
                    graph.add_node(contract.clone());
                    instances.push(CapabilityInstance {
                        contract: contract.clone(),
                        runtime_params: serde_json::json!({}),
                    });
                }
            }
        }

        // 4. Validate Graph Invariants (Dependencies, Conflicts, Cycles)
        graph.validate()?;

        let resolved = ResolvedCapabilitySet { graph, instances };

        // 5. Store in Cache
        self.cache.put(reqs.clone(), resolved.clone());

        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::InMemoryCapabilityRegistry;
    use fusion_plugin_api::CapabilityContract;
    use serde_json::json;

    fn build_test_registry() -> Arc<dyn CapabilityRegistry> {
        let mut reg = InMemoryCapabilityRegistry::new();
        reg.register(CapabilityContract {
            id: CapabilityId::new("echo.text"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Echo text".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 10,
            reliability_score: 1.0,
            supports_streaming: false,
        }).unwrap();

        reg.register(CapabilityContract {
            id: CapabilityId::new("echo.uppercase"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Echo uppercase".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 50,
            reliability_score: 1.0,
            supports_streaming: false,
        }).unwrap();

        reg.freeze();
        Arc::new(reg)
    }

    #[test]
    fn test_capability_resolution_success() {
        let registry = build_test_registry();
        let resolver = CapabilityResolver::new(registry);

        let reqs = RequirementSet::new(vec![CapabilityId::new("echo.text")]);
        let res = resolver.resolve(&reqs).unwrap();

        assert_eq!(res.instances.len(), 1);
        assert_eq!(res.instances[0].contract.id.as_str(), "echo.text");
    }

    #[test]
    fn test_capability_resolution_alias() {
        let registry = build_test_registry();
        let mut resolver = CapabilityResolver::new(registry);
        resolver.register_alias(CapabilityId::new("legacy.echo"), CapabilityId::new("echo.text"));

        let reqs = RequirementSet::new(vec![CapabilityId::new("legacy.echo")]);
        let res = resolver.resolve(&reqs).unwrap();

        assert_eq!(res.instances[0].contract.id.as_str(), "echo.text");
    }

    #[test]
    fn test_capability_resolution_missing_fails() {
        let registry = build_test_registry();
        let resolver = CapabilityResolver::new(registry);

        let reqs = RequirementSet::new(vec![CapabilityId::new("nonexistent.capability")]);
        assert!(resolver.resolve(&reqs).is_err());
    }

    #[test]
    fn test_capability_planner_cache_hit() {
        let registry = build_test_registry();
        let resolver = CapabilityResolver::new(registry);

        let reqs = RequirementSet::new(vec![CapabilityId::new("echo.text")]);
        let _ = resolver.resolve(&reqs).unwrap();
        let res2 = resolver.resolve(&reqs).unwrap(); // Cache hit

        assert_eq!(res2.instances.len(), 1);
    }
}
