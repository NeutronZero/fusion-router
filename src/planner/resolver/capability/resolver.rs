//! Phase 2B & 2C — `CapabilityResolver`, `RequirementSet`, `ResolvedCapabilitySet`, & `CapabilityPlannerCache`
//!
//! Symbol resolution for capabilities, matching intent requirements to frozen contracts.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use parking_lot::Mutex;
use fusion_plugin_api::{CapabilityContract, CapabilityId, CapabilityInstance};
use crate::capability::CapabilityRegistry;
use super::graph::CapabilityGraph;

/// Errors that can occur during capability resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolverError {
    UnregisteredCapability(CapabilityId),
    NoCompatibleVersion { capability: String, requirement: String },
    AmbiguousResolution { capability: String, matches: Vec<CapabilityId> },
    UnresolvedDependency { capability: CapabilityId, dependency: CapabilityId },
    CircularDependency,
    PolicyDenied { capability: CapabilityId, reason: String },
    LatencyExceeded { capability: CapabilityId, latency_ms: u64, max_ms: u64 },
    GraphValidationFailed(String),
}

impl std::fmt::Display for ResolverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolverError::UnregisteredCapability(id) => write!(f, "capability not registered: {id}"),
            ResolverError::NoCompatibleVersion { capability, requirement } => {
                write!(f, "no compatible version for '{capability}' matching '{requirement}'")
            }
            ResolverError::AmbiguousResolution { capability, matches } => {
                write!(f, "ambiguous resolution for '{capability}': matches {matches:?}")
            }
            ResolverError::UnresolvedDependency { capability, dependency } => {
                write!(f, "unresolved dependency: '{capability}' requires '{dependency}'")
            }
            ResolverError::CircularDependency => write!(f, "circular dependency detected"),
            ResolverError::PolicyDenied { capability, reason } => {
                write!(f, "policy denied '{capability}': {reason}")
            }
            ResolverError::LatencyExceeded { capability, latency_ms, max_ms } => {
                write!(f, "capability '{capability}' latency {latency_ms}ms exceeds {max_ms}ms")
            }
            ResolverError::GraphValidationFailed(msg) => write!(f, "graph validation failed: {msg}"),
        }
    }
}

impl std::error::Error for ResolverError {}

/// Context for policy evaluation (stub — used in Task 5).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PolicyContext {
    pub environment: String,
    pub allow_list: Option<Vec<CapabilityId>>,
    pub deny_list: Vec<CapabilityId>,
    pub release_profile: Option<String>,
}

/// A semver version constraint on a capability prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct VersionConstraint {
    pub capability_prefix: String,
    pub requirement: semver::VersionReq,
}

/// Represents extracted intent requirements requesting capability lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RequirementSet {
    pub required_capabilities: Vec<CapabilityId>,
    pub optional_capabilities: Vec<CapabilityId>,
    pub version_constraints: Vec<VersionConstraint>,
    pub max_acceptable_latency_ms: Option<u64>,
    pub max_cost_usd: Option<u64>, // Scaled by 10^6 for integer hashing
    pub policy: Option<PolicyContext>,
}

impl RequirementSet {
    pub fn new(required: Vec<CapabilityId>) -> Self {
        Self {
            required_capabilities: required,
            optional_capabilities: Vec::new(),
            version_constraints: Vec::new(),
            max_acceptable_latency_ms: None,
            max_cost_usd: None,
            policy: None,
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

    /// Resolves a single `VersionConstraint` to the highest-matching `CapabilityContract`.
    fn resolve_version(&self, constraint: &VersionConstraint) -> Result<CapabilityContract, ResolverError> {
        let mut candidates: Vec<&CapabilityContract> = self.registry.list()
            .into_iter()
            .filter(|c| c.id.as_str().starts_with(&constraint.capability_prefix))
            .filter(|c| constraint.requirement.matches(&c.version))
            .collect();

        if candidates.is_empty() {
            return Err(ResolverError::NoCompatibleVersion {
                capability: constraint.capability_prefix.clone(),
                requirement: constraint.requirement.to_string(),
            });
        }

        candidates.sort_by(|a, b| b.version.cmp(&a.version));
        Ok(candidates[0].clone())
    }

    /// Resolves a `RequirementSet` into a `ResolvedCapabilitySet` with dependency graph checks.
    pub fn resolve(&self, reqs: &RequirementSet) -> Result<ResolvedCapabilitySet, ResolverError> {
        // 1. Check Planner Cache
        if let Some(cached) = self.cache.get(reqs) {
            return Ok(cached);
        }

        let mut graph = CapabilityGraph::new();
        let mut instances = Vec::new();
        let mut visited = HashSet::new();

        // 2. Resolve Version Constraints
        for vc in &reqs.version_constraints {
            let contract = self.resolve_version(vc)?;
            if visited.insert(contract.id.clone()) {
                graph.add_node(contract.clone());
                instances.push(CapabilityInstance {
                    contract,
                    runtime_params: serde_json::json!({}),
                });
            }
        }

        // 3. Resolve Required Capabilities
        for req_id in &reqs.required_capabilities {
            let target_id = self.aliases.get(req_id).unwrap_or(req_id);

            let contract = self.registry.get(target_id).ok_or_else(|| {
                ResolverError::UnregisteredCapability(req_id.clone())
            })?;

            // Check latency bound if specified
            if let Some(max_lat) = reqs.max_acceptable_latency_ms {
                if contract.estimated_latency_ms > max_lat {
                    return Err(ResolverError::LatencyExceeded {
                        capability: contract.id.clone(),
                        latency_ms: contract.estimated_latency_ms,
                        max_ms: max_lat,
                    });
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

        // 4. Resolve Optional Capabilities (Graceful Degradation)
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

        // 5. Validate Graph Invariants (Dependencies, Conflicts, Cycles)
        graph.validate().map_err(ResolverError::GraphValidationFailed)?;

        let resolved = ResolvedCapabilitySet { graph, instances };

        // 6. Store in Cache
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
            dependencies: vec![],
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
            dependencies: vec![],
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

    // -----------------------------------------------------------------------
    // SemVer resolution tests
    // -----------------------------------------------------------------------

    fn build_semver_registry() -> Arc<dyn CapabilityRegistry> {
        let mut reg = InMemoryCapabilityRegistry::new();

        for (ver_str, id_suffix) in &[
            ("1.0.0", "v1.0.0"),
            ("1.1.0", "v1.1.0"),
            ("1.2.0", "v1.2.0"),
            ("2.0.0", "v2.0.0"),
        ] {
            reg.register(CapabilityContract {
                id: CapabilityId::new(&format!("echo.{}", id_suffix)),
                version: semver::Version::parse(ver_str).unwrap(),
                description: String::new(),
                inputs_schema: json!({}),
                outputs_schema: json!({}),
                permissions: vec![],
                dependencies: vec![],
                estimated_cost_usd: 0.0,
                estimated_latency_ms: 10,
                reliability_score: 1.0,
                supports_streaming: false,
            }).unwrap();
        }

        reg.freeze();
        Arc::new(reg)
    }

    #[test]
    fn semver_resolves_caret() {
        let registry = build_semver_registry();
        let resolver = CapabilityResolver::new(registry);

        let vc = VersionConstraint {
            capability_prefix: "echo.v".into(),
            requirement: semver::VersionReq::parse("^1.0").unwrap(),
        };

        let contract = resolver.resolve_version(&vc).unwrap();
        assert_eq!(contract.version, semver::Version::new(1, 2, 0));
    }

    #[test]
    fn semver_resolves_tilde() {
        let registry = build_semver_registry();
        let resolver = CapabilityResolver::new(registry);

        let vc = VersionConstraint {
            capability_prefix: "echo.v".into(),
            requirement: semver::VersionReq::parse("~1.0").unwrap(),
        };

        let contract = resolver.resolve_version(&vc).unwrap();
        assert_eq!(contract.version, semver::Version::new(1, 0, 0));
    }

    #[test]
    fn semver_no_compatible_version_fails() {
        let registry = build_semver_registry();
        let resolver = CapabilityResolver::new(registry);

        let vc = VersionConstraint {
            capability_prefix: "echo.v".into(),
            requirement: semver::VersionReq::parse("^3.0").unwrap(),
        };

        let err = resolver.resolve_version(&vc).unwrap_err();
        assert!(matches!(err, ResolverError::NoCompatibleVersion { .. }));
    }
}
