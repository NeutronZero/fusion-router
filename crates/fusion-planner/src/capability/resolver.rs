//! Phase 2B & 2C — `CapabilityResolver`, `RequirementSet`, `ResolvedCapabilitySet`, & `CapabilityPlannerCache`
//!
//! Symbol resolution for capabilities, matching intent requirements to frozen contracts.
//! Ported from the monolith's `src/planner/resolver/capability/resolver.rs`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use fusion_kernel::capability::{CapabilityGraph, CapabilityRegistry, DependencyEdge};
use fusion_plugin_api::{CapabilityContract, CapabilityId, CapabilityInstance};
use parking_lot::Mutex;

/// Errors that can occur during capability resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolverError {
    UnregisteredCapability(CapabilityId),
    NoCompatibleVersion { capability: String, requirement: String },
    #[allow(dead_code)]
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
    pub max_cost: Option<fusion_core::NanoUSD>,
    pub policy: Option<PolicyContext>,
}

impl RequirementSet {
    pub fn new(required: Vec<CapabilityId>) -> Self {
        Self {
            required_capabilities: required,
            optional_capabilities: Vec::new(),
            version_constraints: Vec::new(),
            max_acceptable_latency_ms: None,
            max_cost: None,
            policy: None,
        }
    }
}

/// Output of symbol resolution containing resolved contracts and instantiated runtime handles.
#[derive(Debug, Clone)]
pub struct ResolvedCapabilitySet {
    #[allow(dead_code)]
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

    /// BFS transitive dependency expansion before graph construction.
    /// Every contract added to the result map (root or transitive) is
    /// policy-checked when a policy is present (H13 / ADR-034).
    fn expand_dependencies(
        &self,
        contracts: &[CapabilityContract],
        policy: Option<&PolicyContext>,
    ) -> Result<(Vec<CapabilityContract>, Vec<DependencyEdge>), ResolverError> {
        let mut result_map: HashMap<CapabilityId, Arc<CapabilityContract>> = HashMap::new();
        let mut dep_edges: Vec<DependencyEdge> = Vec::new();
        let mut queue: VecDeque<CapabilityId> = VecDeque::new();

        for c in contracts {
            let id = c.id.clone();
            if let Some(policy) = policy {
                self.apply_policy(&id, c, policy)?;
            }
            result_map.insert(id.clone(), Arc::new(c.clone()));
            queue.push_back(id);
        }

        let mut in_flight: HashSet<CapabilityId> = queue.iter().cloned().collect();

        while let Some(current_id) = queue.pop_front() {
            // Clone the Arc (cheap) so dependencies can be iterated by reference
            // without holding a borrow across the map insert below.
            let current = result_map
                .get(&current_id)
                .ok_or_else(|| ResolverError::UnregisteredCapability(current_id.clone()))?
                .clone();

            for dep_id in &current.dependencies {
                if in_flight.contains(dep_id) {
                    if let Some(dep_contract) = self.registry.get(dep_id) {
                        if dep_contract.dependencies.contains(&current_id) {
                            return Err(ResolverError::CircularDependency);
                        }
                    }
                }

                if !result_map.contains_key(dep_id) {
                    let dep_contract = self.registry.get(dep_id).ok_or_else(|| {
                        ResolverError::UnresolvedDependency {
                            capability: current_id.clone(),
                            dependency: dep_id.clone(),
                        }
                    })?;

                    if let Some(policy) = policy {
                        self.apply_policy(dep_id, dep_contract, policy)?;
                    }

                    result_map.insert(dep_id.clone(), Arc::new(dep_contract.clone()));
                    queue.push_back(dep_id.clone());
                    in_flight.insert(dep_id.clone());
                }

                dep_edges.push(DependencyEdge {
                    from: current_id.clone(),
                    to: dep_id.clone(),
                });
            }
        }

        let mut result: Vec<CapabilityContract> = result_map
            .into_values()
            .map(|contract| contract.as_ref().clone())
            .collect();
        result.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        Ok((result, dep_edges))
    }

    fn apply_policy(
        &self,
        requested: &CapabilityId,
        contract: &CapabilityContract,
        policy: &PolicyContext,
    ) -> Result<(), ResolverError> {
        if policy.deny_list.contains(requested) || policy.deny_list.contains(&contract.id) {
            return Err(ResolverError::PolicyDenied {
                capability: contract.id.clone(),
                reason: "capability is in the deny list".into(),
            });
        }
        if let Some(ref allow) = policy.allow_list {
            if !allow.contains(requested) || !allow.contains(&contract.id) {
                return Err(ResolverError::PolicyDenied {
                    capability: contract.id.clone(),
                    reason: "capability is not in the allow list".into(),
                });
            }
        }
        Ok(())
    }

    /// Resolves a `RequirementSet` into a `ResolvedCapabilitySet` with dependency graph checks.
    pub fn resolve(&self, reqs: &RequirementSet) -> Result<ResolvedCapabilitySet, ResolverError> {
        if let Some(cached) = self.cache.get(reqs) {
            return Ok(cached);
        }

        if let Some(ref policy) = reqs.policy {
            for req_id in &reqs.required_capabilities {
                let target_id = self.aliases.get(req_id).unwrap_or(req_id);
                if let Some(contract) = self.registry.get(target_id) {
                    self.apply_policy(req_id, contract, policy)?;
                }
            }
        }

        let mut instances = Vec::new();
        let mut visited = HashSet::new();

        // Resolve version-constrained requirements
        for vc in &reqs.version_constraints {
            let contract = self.resolve_version(vc)?;
            if let Some(ref policy) = reqs.policy {
                self.apply_policy(&contract.id, &contract, policy)?;
            }
            if visited.insert(contract.id.clone()) {
                instances.push(CapabilityInstance {
                    contract: contract.clone(),
                    runtime_params: serde_json::json!({}),
                });
            }
        }

        // Resolve Required Capabilities (exact ID)
        for req_id in &reqs.required_capabilities {
            let target_id = self.aliases.get(req_id).unwrap_or(req_id);
            let contract = self.registry.get(target_id).ok_or_else(|| {
                ResolverError::UnregisteredCapability(req_id.clone())
            })?;
            if let Some(ref policy) = reqs.policy {
                self.apply_policy(req_id, contract, policy)?;
            }
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
                instances.push(CapabilityInstance {
                    contract: contract.clone(),
                    runtime_params: serde_json::json!({}),
                });
            }
        }

        // Resolve Optional Capabilities
        for opt_id in &reqs.optional_capabilities {
            let target_id = self.aliases.get(opt_id).unwrap_or(opt_id);
            if let Some(contract) = self.registry.get(target_id) {
                if let Some(ref policy) = reqs.policy {
                    self.apply_policy(opt_id, contract, policy)?;
                }
                if visited.insert(contract.id.clone()) {
                    instances.push(CapabilityInstance {
                        contract: contract.clone(),
                        runtime_params: serde_json::json!({}),
                    });
                }
            }
        }

        // Expand transitive dependencies
        let resolved_contracts: Vec<CapabilityContract> = instances.iter().map(|i| i.contract.clone()).collect();
        let (all_contracts, dep_edges) =
            self.expand_dependencies(&resolved_contracts, reqs.policy.as_ref())?;

        // Rebuild graph from expanded set
        let mut graph = CapabilityGraph::new();
        let mut final_instances = Vec::new();
        let mut final_visited = HashSet::new();
        for contract in &all_contracts {
            if final_visited.insert(contract.id.clone()) {
                graph.add_node(contract.clone());
                final_instances.push(CapabilityInstance {
                    contract: contract.clone(),
                    runtime_params: serde_json::json!({}),
                });
            }
        }
        for edge in &dep_edges {
            graph.add_dependency(edge.from.clone(), edge.to.clone());
        }

        graph.validate().map_err(ResolverError::GraphValidationFailed)?;

        // Final belt-and-braces: re-verify every resolved instance against policy
        // (guards against future resolution paths forgetting the check).
        if let Some(ref policy) = reqs.policy {
            for instance in &final_instances {
                self.apply_policy(&instance.contract.id, &instance.contract, policy)?;
            }
        }

        let resolved = ResolvedCapabilitySet { graph, instances: final_instances };
        self.cache.put(reqs.clone(), resolved.clone());
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_kernel::capability::InMemoryCapabilityRegistry;
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
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 10,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }).unwrap();

        reg.register(CapabilityContract {
            id: CapabilityId::new("echo.uppercase"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Echo uppercase".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 50,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
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
                id: CapabilityId::new(format!("echo.{}", id_suffix)),
                version: semver::Version::parse(ver_str).unwrap(),
                description: String::new(),
                inputs_schema: json!({}),
                outputs_schema: json!({}),
                permissions: vec![],
                dependencies: vec![],
                estimated_cost: fusion_core::NanoUSD::ZERO,
                estimated_latency_ms: 10,
                reliability_score: 1.0,
                supports_streaming: false,
                traits: vec![],
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

    // -----------------------------------------------------------------------
    // Dependency expansion tests
    // -----------------------------------------------------------------------

    fn build_registry_with_deps() -> Arc<dyn CapabilityRegistry> {
        let mut reg = InMemoryCapabilityRegistry::new();

        reg.register(CapabilityContract {
            id: CapabilityId::new("cap.shell"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Shell access".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 5,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }).unwrap();

        reg.register(CapabilityContract {
            id: CapabilityId::new("cap.filesystem"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Filesystem access".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![CapabilityId::new("cap.shell")],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 10,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }).unwrap();

        reg.register(CapabilityContract {
            id: CapabilityId::new("cap.browser"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Browser access".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![CapabilityId::new("cap.filesystem")],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 20,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }).unwrap();

        reg.freeze();
        Arc::new(reg)
    }

    #[test]
    fn expands_transitive_dependencies() {
        let registry = build_registry_with_deps();
        let resolver = CapabilityResolver::new(registry);

        let reqs = RequirementSet::new(vec![CapabilityId::new("cap.browser")]);
        let res = resolver.resolve(&reqs).unwrap();

        assert_eq!(res.instances.len(), 3);
        let ids: Vec<&str> = res.instances.iter().map(|i| i.contract.id.as_str()).collect();
        assert!(ids.contains(&"cap.browser"));
        assert!(ids.contains(&"cap.filesystem"));
        assert!(ids.contains(&"cap.shell"));
    }

    #[test]
    fn deduplicates_dependencies() {
        let registry = build_registry_with_deps();
        let resolver = CapabilityResolver::new(registry);

        let reqs = RequirementSet::new(vec![
            CapabilityId::new("cap.browser"),
            CapabilityId::new("cap.filesystem"),
        ]);
        let res = resolver.resolve(&reqs).unwrap();

        assert_eq!(res.instances.len(), 3);
    }

    #[test]
    fn unresolved_dependency_fails() {
        let mut reg = InMemoryCapabilityRegistry::new();
        reg.register(CapabilityContract {
            id: CapabilityId::new("cap.broken"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Broken cap".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![CapabilityId::new("cap.missing")],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 10,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }).unwrap();
        reg.freeze();
        let registry: Arc<dyn CapabilityRegistry> = Arc::new(reg);
        let resolver = CapabilityResolver::new(registry);

        let reqs = RequirementSet::new(vec![CapabilityId::new("cap.broken")]);
        let err = resolver.resolve(&reqs).unwrap_err();
        assert!(matches!(err, ResolverError::UnresolvedDependency { .. }));
    }

    // -----------------------------------------------------------------------
    // Policy constraint tests
    // -----------------------------------------------------------------------

    #[test]
    fn deny_list_excludes_capability() {
        let registry = build_test_registry();
        let resolver = CapabilityResolver::new(registry);

        let mut reqs = RequirementSet::new(vec![CapabilityId::new("echo.text")]);
        reqs.policy = Some(PolicyContext {
            environment: "test".into(),
            allow_list: None,
            deny_list: vec![CapabilityId::new("echo.text")],
            release_profile: None,
        });

        let err = resolver.resolve(&reqs).unwrap_err();
        assert!(matches!(err, ResolverError::PolicyDenied { .. }));
    }

    #[test]
    fn allow_list_restricts_to_specific() {
        let registry = build_test_registry();
        let resolver = CapabilityResolver::new(registry);

        let mut reqs = RequirementSet::new(vec![CapabilityId::new("echo.uppercase")]);
        reqs.policy = Some(PolicyContext {
            environment: "test".into(),
            allow_list: Some(vec![CapabilityId::new("echo.text")]),
            deny_list: vec![],
            release_profile: None,
        });

        let err = resolver.resolve(&reqs).unwrap_err();
        assert!(matches!(err, ResolverError::PolicyDenied { .. }));
    }

    #[test]
    fn policy_pass_allows_resolution() {
        let registry = build_test_registry();
        let resolver = CapabilityResolver::new(registry);

        let mut reqs = RequirementSet::new(vec![CapabilityId::new("echo.text")]);
        reqs.policy = Some(PolicyContext {
            environment: "test".into(),
            allow_list: Some(vec![CapabilityId::new("echo.text")]),
            deny_list: vec![],
            release_profile: None,
        });

        let res = resolver.resolve(&reqs).unwrap();
        assert_eq!(res.instances.len(), 1);
        assert_eq!(res.instances[0].contract.id.as_str(), "echo.text");
    }

    #[test]
    fn deny_list_rejects_version_constrained_capability() {
        let registry = build_semver_registry();
        let resolver = CapabilityResolver::new(registry);

        let mut reqs = RequirementSet::new(vec![]);
        reqs.version_constraints = vec![VersionConstraint {
            capability_prefix: "echo.v".into(),
            requirement: semver::VersionReq::parse("^1.0").unwrap(),
        }];
        reqs.policy = Some(PolicyContext {
            environment: "test".into(),
            allow_list: None,
            deny_list: vec![CapabilityId::new("echo.v1.2.0")],
            release_profile: None,
        });

        let err = resolver.resolve(&reqs).unwrap_err();
        assert!(matches!(err, ResolverError::PolicyDenied { .. }));
    }

    #[test]
    fn deny_list_rejects_optional_capability() {
        let registry = build_test_registry();
        let resolver = CapabilityResolver::new(registry);

        let mut reqs = RequirementSet::new(vec![CapabilityId::new("echo.text")]);
        reqs.optional_capabilities = vec![CapabilityId::new("echo.uppercase")];
        reqs.policy = Some(PolicyContext {
            environment: "test".into(),
            allow_list: None,
            deny_list: vec![CapabilityId::new("echo.uppercase")],
            release_profile: None,
        });

        let err = resolver.resolve(&reqs).unwrap_err();
        assert!(matches!(err, ResolverError::PolicyDenied { .. }));
    }

    #[test]
    fn deny_list_rejects_transitive_dependency() {
        let registry = build_registry_with_deps();
        let resolver = CapabilityResolver::new(registry);

        let mut reqs = RequirementSet::new(vec![CapabilityId::new("cap.browser")]);
        reqs.policy = Some(PolicyContext {
            environment: "test".into(),
            allow_list: None,
            deny_list: vec![CapabilityId::new("cap.shell")],
            release_profile: None,
        });

        let err = resolver.resolve(&reqs).unwrap_err();
        assert!(matches!(err, ResolverError::PolicyDenied { .. }));
    }

    #[test]
    fn allow_list_allows_transitive_dependency() {
        let registry = build_registry_with_deps();
        let resolver = CapabilityResolver::new(registry);

        let mut reqs = RequirementSet::new(vec![CapabilityId::new("cap.browser")]);
        reqs.policy = Some(PolicyContext {
            environment: "test".into(),
            allow_list: Some(vec![
                CapabilityId::new("cap.browser"),
                CapabilityId::new("cap.filesystem"),
                CapabilityId::new("cap.shell"),
            ]),
            deny_list: vec![],
            release_profile: None,
        });

        let res = resolver.resolve(&reqs).unwrap();
        assert_eq!(res.instances.len(), 3);
    }

    #[test]
    fn allow_list_rejects_non_allowlisted_transitive_dependency() {
        let registry = build_registry_with_deps();
        let resolver = CapabilityResolver::new(registry);

        let mut reqs = RequirementSet::new(vec![CapabilityId::new("cap.browser")]);
        reqs.policy = Some(PolicyContext {
            environment: "test".into(),
            allow_list: Some(vec![
                CapabilityId::new("cap.browser"),
                CapabilityId::new("cap.filesystem"),
            ]),
            deny_list: vec![],
            release_profile: None,
        });

        let err = resolver.resolve(&reqs).unwrap_err();
        assert!(matches!(err, ResolverError::PolicyDenied { .. }));
    }

    // -----------------------------------------------------------------------
    // Equivalence tests — cycle detection (two independent mechanisms)
    // -----------------------------------------------------------------------

    /// Graph-level cycle detection: Kahn's algorithm in `CapabilityGraph::validate()`.
    /// A 3-node cycle (A → B → C → A) has no direct reverse edge between adjacent
    /// nodes, so the resolver-level BFS in-flight check won't catch it — only
    /// the graph-level topological sort will.
    #[test]
    fn cycle_detection_graph_level_kahns() {
        let mut reg = InMemoryCapabilityRegistry::new();

        // A depends on B
        reg.register(CapabilityContract {
            id: CapabilityId::new("cap.a"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "A".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![CapabilityId::new("cap.b")],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 10,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }).unwrap();

        // B depends on C
        reg.register(CapabilityContract {
            id: CapabilityId::new("cap.b"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "B".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![CapabilityId::new("cap.c")],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 10,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }).unwrap();

        // C depends on A — completing the cycle
        reg.register(CapabilityContract {
            id: CapabilityId::new("cap.c"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "C".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![CapabilityId::new("cap.a")],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 10,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }).unwrap();

        reg.freeze();
        let registry: Arc<dyn CapabilityRegistry> = Arc::new(reg);
        let resolver = CapabilityResolver::new(registry);

        let reqs = RequirementSet::new(vec![CapabilityId::new("cap.a")]);
        let err = resolver.resolve(&reqs).unwrap_err();

        // The BFS in-flight check won't catch this because there's no direct
        // reverse edge (A→B, B→C, C→A — no adjacent pair has mutual dependency).
        // The graph-level Kahn's algorithm catches it via topological sort failure.
        assert!(
            matches!(err, ResolverError::GraphValidationFailed(ref msg) if msg.contains("Cyclic dependency")),
            "Expected GraphValidationFailed from Kahn's algorithm, got: {:?}",
            err
        );
    }

    /// Resolver-level cycle detection: BFS in-flight set + reverse-edge check.
    /// A 2-node cycle (A ↔ B) has a direct reverse edge, so the BFS catches it
    /// during dependency expansion before the graph is even constructed.
    #[test]
    fn cycle_detection_resolver_bfs_inflight() {
        let mut reg = InMemoryCapabilityRegistry::new();

        // A depends on B
        reg.register(CapabilityContract {
            id: CapabilityId::new("cap.x"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "X".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![CapabilityId::new("cap.y")],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 10,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }).unwrap();

        // B depends on A — direct reverse edge
        reg.register(CapabilityContract {
            id: CapabilityId::new("cap.y"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Y".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![CapabilityId::new("cap.x")],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 10,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }).unwrap();

        reg.freeze();
        let registry: Arc<dyn CapabilityRegistry> = Arc::new(reg);
        let resolver = CapabilityResolver::new(registry);

        let reqs = RequirementSet::new(vec![CapabilityId::new("cap.x")]);
        let err = resolver.resolve(&reqs).unwrap_err();

        // The BFS in-flight check catches this: when processing Y's dependency on X,
        // X is already in in_flight AND X.dependencies contains Y (reverse edge).
        assert!(
            matches!(err, ResolverError::CircularDependency),
            "Expected CircularDependency from BFS in-flight check, got: {:?}",
            err
        );
    }

    // -----------------------------------------------------------------------
    // Equivalence tests — PolicyDenied at each check point
    // -----------------------------------------------------------------------

    /// Check point 1: Required capabilities pre-check.
    /// A required capability in the deny-list is rejected before any resolution.
    #[test]
    fn policy_denied_check_point_1_required_precheck() {
        let registry = build_test_registry();
        let resolver = CapabilityResolver::new(registry);

        let mut reqs = RequirementSet::new(vec![CapabilityId::new("echo.text")]);
        reqs.policy = Some(PolicyContext {
            environment: "test".into(),
            allow_list: None,
            deny_list: vec![CapabilityId::new("echo.text")],
            release_profile: None,
        });

        let err = resolver.resolve(&reqs).unwrap_err();
        assert!(
            matches!(err, ResolverError::PolicyDenied { ref capability, .. } if capability.as_str() == "echo.text"),
            "Check point 1 should reject required capability in deny-list, got: {:?}",
            err
        );
    }

    /// Check point 2: Version-constrained resolution.
    /// A version-constrained capability in the deny-list is rejected after
    /// `resolve_version()` succeeds but before it's added to instances.
    #[test]
    fn policy_denied_check_point_2_version_constrained() {
        let registry = build_semver_registry();
        let resolver = CapabilityResolver::new(registry);

        let mut reqs = RequirementSet::new(vec![]);
        reqs.version_constraints = vec![VersionConstraint {
            capability_prefix: "echo.v".into(),
            requirement: semver::VersionReq::parse("^1.0").unwrap(),
        }];
        // Deny the highest matching version (1.2.0)
        reqs.policy = Some(PolicyContext {
            environment: "test".into(),
            allow_list: None,
            deny_list: vec![CapabilityId::new("echo.v1.2.0")],
            release_profile: None,
        });

        let err = resolver.resolve(&reqs).unwrap_err();
        assert!(
            matches!(err, ResolverError::PolicyDenied { ref capability, .. } if capability.as_str() == "echo.v1.2.0"),
            "Check point 2 should reject version-constrained capability in deny-list, got: {:?}",
            err
        );
    }

    /// Check point 4: Belt-and-braces re-verification (defense-in-depth).
    ///
    /// This check re-verifies every resolved instance against policy AFTER graph
    /// construction (lines 370-374). In the current implementation, points 1-3
    /// already catch all denied capabilities — every path into `final_instances`
    /// flows through `expand_dependencies`, which checks policy for all initial
    /// contracts and transitive dependencies. There is no current code path where
    /// point 4 is the *only* check that catches a denial.
    ///
    /// This test exercises the full resolution pipeline (required + transitive
    /// dependency + belt-and-braces) with a deny-listed transitive dependency to
    /// prove the belt-and-braces code path *runs* as part of the happy path for
    /// allowed capabilities. A future refactor that removes or bypasses the
    /// belt-and-braces check would still pass this test (because point 3 catches
    /// the denial), but the test documents the intended contract: every resolved
    /// instance is re-verified after graph construction.
    ///
    /// If a future code change adds a resolution path that bypasses points 1-3,
    /// this test should be extended with a case where the denied capability enters
    /// `final_instances` only through that new path, proving defense-in-depth.
    #[test]
    fn policy_denied_check_point_4_belt_and_braces() {
        let registry = build_registry_with_deps();
        let resolver = CapabilityResolver::new(registry);

        // cap.browser → cap.filesystem → cap.shell
        // Deny cap.shell (transitive dependency only, not in required_capabilities)
        let mut reqs = RequirementSet::new(vec![CapabilityId::new("cap.browser")]);
        reqs.policy = Some(PolicyContext {
            environment: "test".into(),
            allow_list: None,
            deny_list: vec![CapabilityId::new("cap.shell")],
            release_profile: None,
        });

        let err = resolver.resolve(&reqs).unwrap_err();
        // Caught by point 3 (transitive expansion), not point 4.
        // Point 4 would also catch it if it ran, but point 3 returns first.
        assert!(
            matches!(err, ResolverError::PolicyDenied { ref capability, .. } if capability.as_str() == "cap.shell"),
            "Transitive dependency in deny-list should be rejected, got: {:?}",
            err
        );
    }

    // -----------------------------------------------------------------------
    // Equivalence tests — LatencyExceeded scoping
    // -----------------------------------------------------------------------

    /// LatencyExceeded for a required capability: must reject.
    /// The latency check is scoped to required capabilities only (lines 311-319).
    #[test]
    fn latency_exceeded_required_rejects() {
        let mut reg = InMemoryCapabilityRegistry::new();

        // echo.uppercase has 50ms latency
        reg.register(CapabilityContract {
            id: CapabilityId::new("echo.uppercase"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Echo uppercase".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 50,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }).unwrap();

        reg.freeze();
        let registry: Arc<dyn CapabilityRegistry> = Arc::new(reg);
        let resolver = CapabilityResolver::new(registry);

        let mut reqs = RequirementSet::new(vec![CapabilityId::new("echo.uppercase")]);
        reqs.max_acceptable_latency_ms = Some(30); // 30ms < 50ms

        let err = resolver.resolve(&reqs).unwrap_err();
        assert!(
            matches!(err, ResolverError::LatencyExceeded { ref capability, latency_ms, max_ms }
                if capability.as_str() == "echo.uppercase" && latency_ms == 50 && max_ms == 30),
            "Required capability exceeding latency should be rejected, got: {:?}",
            err
        );
    }

    /// LatencyExceeded for an optional capability: must be silently allowed.
    /// The latency check is scoped to required capabilities only — optional
    /// capabilities that exceed the threshold are silently included in results.
    /// This is an explicit "nothing happens" assertion to catch regressions
    /// where a refactor accidentally extends the latency check to optional caps.
    #[test]
    fn latency_exceeded_optional_silently_allowed() {
        let mut reg = InMemoryCapabilityRegistry::new();

        // echo.text has 10ms latency
        reg.register(CapabilityContract {
            id: CapabilityId::new("echo.text"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Echo text".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 10,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }).unwrap();

        // echo.uppercase has 50ms latency
        reg.register(CapabilityContract {
            id: CapabilityId::new("echo.uppercase"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "Echo uppercase".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost: fusion_core::NanoUSD::ZERO,
            estimated_latency_ms: 50,
            reliability_score: 1.0,
            supports_streaming: false,
            traits: vec![],
        }).unwrap();

        reg.freeze();
        let registry: Arc<dyn CapabilityRegistry> = Arc::new(reg);
        let resolver = CapabilityResolver::new(registry);

        // Required: echo.text (10ms), Optional: echo.uppercase (50ms)
        // Max latency: 30ms — echo.text passes, echo.uppercase exceeds but is optional
        let mut reqs = RequirementSet::new(vec![CapabilityId::new("echo.text")]);
        reqs.optional_capabilities = vec![CapabilityId::new("echo.uppercase")];
        reqs.max_acceptable_latency_ms = Some(30);

        let res = resolver.resolve(&reqs).unwrap();

        // Optional capability exceeding latency is silently included — no error
        assert_eq!(res.instances.len(), 2, "Both required and optional should be resolved");
        let ids: Vec<&str> = res.instances.iter().map(|i| i.contract.id.as_str()).collect();
        assert!(ids.contains(&"echo.text"), "Required cap should be present");
        assert!(ids.contains(&"echo.uppercase"), "Optional cap exceeding latency should still be present");
    }
}