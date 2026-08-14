# Sprint O2.5 — Capability Resolution Bridge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the existing `CapabilityResolver` with semver resolution, dependency expansion, policy constraints, and deterministic `CapabilityGraph` → `ExecutionGraph` lowering.

**Architecture:** Four additive extensions to the existing resolution layer — (1) `dependencies` field on `CapabilityContract` ABI, (2) semver resolution + `ResolverError`, (3) BFS dependency expansion, (4) policy constraints, (5) `CapabilityGraphLowerer` — all preserving the existing `CapabilityGraph`, `CapabilityResolver`, and registry.

**Tech Stack:** Rust 2021, `semver` crate for version matching, `uuid` for deterministic node IDs, existing `ExecutionGraph` types in `src/types/mod.rs`

## Global Constraints

- No rewrites of `CapabilityGraph`, `CapabilityResolver`, `CapabilityRegistry`, `CapabilityPlannerCache`
- All existing cycle/conflict detection, alias resolution, and caching preserved
- `CapabilityContract.dependencies` is `Vec<CapabilityId>` (ABI field)
- `resolve()` return type changes from `Result<_, String>` to `Result<_, ResolverError>`
- Lowering is deterministic: same input → same `ExecutionGraph`
- Policy is a selection concern only — evaluated before graph construction
- Dependency expansion happens before graph construction (BFS)
- Zero warnings — `cargo check` + `cargo clippy --all-targets -- -D warnings` clean
- All existing tests pass

---

## File Structure

| File | Responsibility |
|------|---------------|
| **Modify:** `crates/fusion-plugin-api/src/lib.rs` | Add `dependencies: Vec<CapabilityId>` to `CapabilityContract` |
| **Modify:** `src/planner/resolver/capability/resolver.rs` | Add `ResolverError`, `VersionConstraint`, `PolicyContext`; extend `RequirementSet`; implement semver matching, dependency expansion, policy filtering |
| **Create:** `src/planner/resolver/capability/lowerer.rs` | `CapabilityGraphLowerer` — deterministic `CapabilityGraph` → `ExecutionGraph` lowering |
| **Modify:** `src/planner/resolver/capability/mod.rs` | Re-export new types and module |
| **Modify:** `tests/unit/phase_invariants.rs` | Add deterministic lowering and resolver error invariants |
| **Modify:** 18 files with `CapabilityContract` construction | Add `dependencies: vec![]` field |

---

### Task 1: Add `dependencies` field to `CapabilityContract` (ABI)

**Files:**
- Modify: `crates/fusion-plugin-api/src/lib.rs` (struct definition)
- Modify: All 25 `CapabilityContract` construction sites across 18 files

**Interfaces:**
- Consumes: existing `CapabilityContract` struct
- Produces: `CapabilityContract` with new `dependencies: Vec<CapabilityId>` field

- [ ] **Step 1: Add field to struct definition**

In `crates/fusion-plugin-api/src/lib.rs`, add `dependencies` after `permissions`:

```rust
pub struct CapabilityContract {
    pub id: CapabilityId,
    pub version: semver::Version,
    pub description: String,
    pub inputs_schema: serde_json::Value,
    pub outputs_schema: serde_json::Value,
    pub permissions: Vec<Permission>,
    pub dependencies: Vec<CapabilityId>,  // NEW
    pub estimated_cost_usd: f64,
    pub estimated_latency_ms: u64,
    pub reliability_score: f32,
    pub supports_streaming: bool,
}
```

- [ ] **Step 2: Run compile check to see all broken sites**

Run: `cargo check 2>&1 | Select-String "missing field"`

Expected: ~25 errors about `missing field 'dependencies'`

- [ ] **Step 3: Add `dependencies: vec![]` to every construction site**

For each file listed below, insert `dependencies: vec![],` after the `permissions:` line in every `CapabilityContract { ... }` literal:

**Source files (12 files):**
- `crates/fusion-plugin-api/src/lib.rs:254`
- `crates/fusion-capability-sdk/src/builder.rs:80`
- `crates/fusion-capability-macros/src/lib.rs:92`
- `src/types/execution_context.rs:159`
- `src/executor/capability_executor.rs:113`
- `src/connectors/shell.rs:29`
- `src/connectors/mcp.rs:27`
- `src/connectors/http.rs:27`
- `src/connectors/github.rs:27`
- `src/connectors/filesystem.rs:27`
- `src/connectors/browser.rs:27`
- `src/scheduler/connector_resolver.rs:167`
- `src/planner/resolver/capability/resolver.rs:165, 178`
- `src/planner/resolver/capability/graph.rs:166-167`
- `src/capability/registry.rs:140, 164, 179, 200, 222, 234, 257`

**Test files (4 files):**
- `tests/unit/phase_invariants.rs:13, 55, 80, 92`
- `tests/unit/session_phase_invariants.rs:21`
- `tests/unit/runtime_phase_invariants.rs:41`
- `tests/replay/mod.rs:14`

**Plugin files (1 file):**
- `plugins/fusion-plugin-echo/src/lib.rs:41, 64`

Edit pattern for each site. Change this:

```rust
            permissions: vec![],
            estimated_cost_usd: 0.0,
```

To this:

```rust
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
```

- [ ] **Step 4: Compile check**

Run: `cargo check`
Expected: Clean compilation, no `missing field 'dependencies'` errors

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(abi): add dependencies field to CapabilityContract"
```

---

### Task 2: `ResolverError` type and `resolve()` return type migration

**Files:**
- Modify: `src/planner/resolver/capability/resolver.rs`
- Modify: `src/planner/resolver/capability/mod.rs`

**Interfaces:**
- Consumes: existing `CapabilityResolver::resolve()` returning `Result<ResolvedCapabilitySet, String>`
- Produces: `ResolverError` enum; `resolve()` now returns `Result<ResolvedCapabilitySet, ResolverError>`

- [ ] **Step 1: Add `ResolverError` enum to `resolver.rs`**

Add after `use` imports and before `RequirementSet`:

```rust
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
```

- [ ] **Step 2: Change `resolve()` return type**

Replace the existing `resolve()` signature:

```rust
    pub fn resolve(&self, reqs: &RequirementSet) -> Result<ResolvedCapabilitySet, ResolverError> {
```

Update all `Err(format!(...))` calls to use `ResolverError` variants:

Line 107-108 — change:
```rust
            let contract = self.registry.get(target_id).ok_or_else(|| {
                format!("Capability resolution failed: required capability '{}' not registered", req_id)
            })?;
```
To:
```rust
            let contract = self.registry.get(target_id).ok_or_else(|| {
                ResolverError::UnregisteredCapability(req_id.clone())
            })?;
```

Lines 113-118 — change:
```rust
                    return Err(format!(
                        "Capability '{}' latency ({}ms) exceeds requirement limit ({}ms)",
                        contract.id, contract.estimated_latency_ms, max_lat
                    ));
```
To:
```rust
                    return Err(ResolverError::LatencyExceeded {
                        capability: contract.id.clone(),
                        latency_ms: contract.estimated_latency_ms,
                        max_ms: max_lat,
                    });
```

Line 144 — change `graph.validate()?` — this returns `Result<(), String>`. Wrap it:
```rust
        graph.validate().map_err(|e| ResolverError::GraphValidationFailed(e))?;
```

- [ ] **Step 3: Update `mod.rs` exports**

In `src/planner/resolver/capability/mod.rs`, add `ResolverError` to the exports:

```rust
pub use resolver::{
    CapabilityResolver, CapabilityPlannerCache, RequirementSet,
    ResolvedCapabilitySet, ResolverError,
};
```

- [ ] **Step 4: Update test expectations**

In the resolver tests, change `assert!(resolver.resolve(&reqs).is_err())` — this still works since `is_err()` is on `Result` regardless of error type.

In `tests/unit/phase_invariants.rs`, update any code that pattern-matches on `Err(String)` to use `ResolverError`.

- [ ] **Step 5: Compile and test**

Run: `cargo check && cargo test -p fusion-router`
Expected: Clean compilation, all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/planner/resolver/capability/resolver.rs src/planner/resolver/capability/mod.rs
git commit -m "feat(capability): add ResolverError type, migrate resolve() to typed errors"
```

---

### Task 3: SemVer resolution

**Files:**
- Modify: `src/planner/resolver/capability/resolver.rs`

**Interfaces:**
- Consumes: `CapabilityRegistry::list()` returning all available contracts
- Produces: `VersionConstraint`, extended `RequirementSet`, semver matching logic

- [ ] **Step 1: Write failing tests for semver resolution**

Append to the `#[cfg(test)] mod tests` block in `resolver.rs`:

```rust
    fn registry_with_versions() -> Arc<dyn CapabilityRegistry> {
        let mut reg = InMemoryCapabilityRegistry::new();
        for (ver, lat) in [("1.0.0", 10), ("1.1.0", 15), ("1.2.0", 20), ("2.0.0", 30)] {
            reg.register(CapabilityContract {
                id: CapabilityId::new(format!("echo.v{ver}")),
                version: semver::Version::parse(ver).unwrap(),
                description: format!("echo {ver}"),
                inputs_schema: json!({}),
                outputs_schema: json!({}),
                permissions: vec![],
                dependencies: vec![],
                estimated_cost_usd: 0.0,
                estimated_latency_ms: lat,
                reliability_score: 1.0,
                supports_streaming: false,
            }).unwrap();
        }
        reg.freeze();
        Arc::new(reg)
    }

    #[test]
    fn semver_resolves_caret() {
        let registry = registry_with_versions();
        let resolver = CapabilityResolver::new(registry);
        let reqs = RequirementSet {
            required_capabilities: vec![],
            optional_capabilities: vec![],
            version_constraints: vec![VersionConstraint {
                capability_prefix: "echo".into(),
                requirement: semver::VersionReq::parse("^1.0").unwrap(),
            }],
            max_acceptable_latency_ms: None,
            max_cost_usd: None,
            policy: None,
        };
        let res = resolver.resolve(&reqs).unwrap();
        assert_eq!(res.instances.len(), 1);
        assert_eq!(res.instances[0].contract.version, semver::Version::parse("1.2.0").unwrap());
    }

    #[test]
    fn semver_resolves_tilde() {
        let registry = registry_with_versions();
        let resolver = CapabilityResolver::new(registry);
        let reqs = RequirementSet {
            required_capabilities: vec![],
            optional_capabilities: vec![],
            version_constraints: vec![VersionConstraint {
                capability_prefix: "echo".into(),
                requirement: semver::VersionReq::parse("~1.0").unwrap(),
            }],
            max_acceptable_latency_ms: None,
            max_cost_usd: None,
            policy: None,
        };
        let res = resolver.resolve(&reqs).unwrap();
        assert_eq!(res.instances[0].contract.version, semver::Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn semver_no_compatible_version_fails() {
        let registry = registry_with_versions();
        let resolver = CapabilityResolver::new(registry);
        let reqs = RequirementSet {
            required_capabilities: vec![],
            optional_capabilities: vec![],
            version_constraints: vec![VersionConstraint {
                capability_prefix: "echo".into(),
                requirement: semver::VersionReq::parse("^3.0").unwrap(),
            }],
            max_acceptable_latency_ms: None,
            max_cost_usd: None,
            policy: None,
        };
        match resolver.resolve(&reqs) {
            Err(ResolverError::NoCompatibleVersion { .. }) => {}
            _ => panic!("expected NoCompatibleVersion error"),
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p fusion-router -- capability::resolver::tests`
Expected: Compile errors — `VersionConstraint` and `RequirementSet` fields not defined

- [ ] **Step 3: Add `VersionConstraint` struct**

Add before `RequirementSet`:

```rust
/// A version-constrained capability requirement (prefix + semver range).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct VersionConstraint {
    pub capability_prefix: String,
    pub requirement: semver::VersionReq,
}
```

- [ ] **Step 4: Extend `RequirementSet`**

Add `version_constraints` and `policy` fields:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RequirementSet {
    pub required_capabilities: Vec<CapabilityId>,
    pub optional_capabilities: Vec<CapabilityId>,
    pub version_constraints: Vec<VersionConstraint>,
    pub max_acceptable_latency_ms: Option<u64>,
    pub max_cost_usd: Option<u64>,
    pub policy: Option<PolicyContext>,
}
```

Update `RequirementSet::new()`:
```rust
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
```

- [ ] **Step 5: Add `PolicyContext` stub (for Task 5, needed here for field completeness)**

Add before `VersionConstraint`:

```rust
/// Policy context for capability selection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PolicyContext {
    pub environment: String,
    pub allow_list: Option<Vec<CapabilityId>>,
    pub deny_list: Vec<CapabilityId>,
    pub release_profile: Option<String>,
}
```

- [ ] **Step 6: Add `semver` dependency if not already present**

Check if `semver` is in the workspace `Cargo.toml`:
Run: `Select-String -Path "Cargo.toml" -Pattern "semver"`

The resolver already uses `semver::Version`, so the dependency is present. No action needed.

- [ ] **Step 7: Implement version matching in `resolve()`**

Add this method to `CapabilityResolver`:

```rust
    /// Resolves a version-constrained requirement to the best matching contract.
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

        // Pick highest version
        candidates.sort_by(|a, b| b.version.cmp(&a.version));
        Ok(candidates[0].clone())
    }
```

Insert version constraint resolution at the start of `resolve()`, before the required cap loop:

```rust
    pub fn resolve(&self, reqs: &RequirementSet) -> Result<ResolvedCapabilitySet, ResolverError> {
        if let Some(cached) = self.cache.get(reqs) {
            return Ok(cached);
        }

        let mut graph = CapabilityGraph::new();
        let mut instances = Vec::new();
        let mut visited = HashSet::new();

        // 2a. Resolve version-constrained requirements
        for vc in &reqs.version_constraints {
            let contract = self.resolve_version(vc)?;
            if visited.insert(contract.id.clone()) {
                graph.add_node(contract.clone());
                instances.push(CapabilityInstance {
                    contract: contract.clone(),
                    runtime_params: serde_json::json!({}),
                });
            }
        }

        // 2b. Resolve Required Capabilities (exact ID, unchanged)
        for req_id in &reqs.required_capabilities {
            ...
```

- [ ] **Step 8: Run tests**

Run: `cargo test -p fusion-router -- capability::resolver::tests`
Expected: 3 new tests pass, all existing tests pass

- [ ] **Step 9: Commit**

```bash
git add src/planner/resolver/capability/resolver.rs
git commit -m "feat(capability): add semver resolution with VersionConstraint"
```

---

### Task 4: Dependency expansion (BFS)

**Files:**
- Modify: `src/planner/resolver/capability/resolver.rs`

**Interfaces:**
- Consumes: `CapabilityContract.dependencies` (from Task 1), `CapabilityRegistry`
- Produces: `expand_dependencies()` BFS method on `CapabilityResolver`

- [ ] **Step 1: Write failing tests for dependency expansion**

Append to the `#[cfg(test)] mod tests` block:

```rust
    fn registry_with_deps() -> Arc<dyn CapabilityRegistry> {
        let mut reg = InMemoryCapabilityRegistry::new();
        // shell <-- filesystem <-- browser
        reg.register(CapabilityContract {
            id: CapabilityId::new("shell"),
            version: semver::Version::parse("1.0.0").unwrap(),
            description: "shell".into(),
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
            id: CapabilityId::new("filesystem"),
            version: semver::Version::parse("1.0.0").unwrap(),
            description: "filesystem".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![CapabilityId::new("shell")],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 20,
            reliability_score: 1.0,
            supports_streaming: false,
        }).unwrap();
        reg.register(CapabilityContract {
            id: CapabilityId::new("browser"),
            version: semver::Version::parse("1.0.0").unwrap(),
            description: "browser".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![CapabilityId::new("filesystem")],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 30,
            reliability_score: 1.0,
            supports_streaming: false,
        }).unwrap();
        reg.freeze();
        Arc::new(reg)
    }

    #[test]
    fn expands_transitive_dependencies() {
        let registry = registry_with_deps();
        let resolver = CapabilityResolver::new(registry);
        let reqs = RequirementSet::new(vec![CapabilityId::new("browser")]);
        let res = resolver.resolve(&reqs).unwrap();
        // Should include browser + filesystem + shell
        assert_eq!(res.instances.len(), 3);
        let ids: Vec<&str> = res.instances.iter().map(|i| i.contract.id.as_str()).collect();
        assert!(ids.contains(&"browser"));
        assert!(ids.contains(&"filesystem"));
        assert!(ids.contains(&"shell"));
    }

    #[test]
    fn deduplicates_dependencies() {
        let registry = registry_with_deps();
        let resolver = CapabilityResolver::new(registry);
        let reqs = RequirementSet::new(vec![
            CapabilityId::new("browser"),
            CapabilityId::new("filesystem"), // same dep, explicit
        ]);
        let res = resolver.resolve(&reqs).unwrap();
        assert_eq!(res.instances.len(), 3); // not 4
    }

    #[test]
    fn unresolved_dependency_fails() {
        let mut reg = InMemoryCapabilityRegistry::new();
        reg.register(CapabilityContract {
            id: CapabilityId::new("a"),
            version: semver::Version::parse("1.0.0").unwrap(),
            description: "a".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![CapabilityId::new("missing")],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 10,
            reliability_score: 1.0,
            supports_streaming: false,
        }).unwrap();
        reg.freeze();
        let resolver = CapabilityResolver::new(Arc::new(reg));
        let reqs = RequirementSet::new(vec![CapabilityId::new("a")]);
        match resolver.resolve(&reqs) {
            Err(ResolverError::UnresolvedDependency { .. }) => {}
            other => panic!("expected UnresolvedDependency, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p fusion-router -- capability::resolver::tests`
Expected: Compile error at test 1 (`browser` uses `CapabilityId::new("shell")` which works) — actually tests should compile. The first expansion test should fail because `expand_dependencies` doesn't exist yet.

- [ ] **Step 3: Implement `expand_dependencies()` on `CapabilityResolver`**

```rust
    /// Expands transitive dependencies via BFS starting from a set of contracts.
    /// Returns all contracts (initial + transitive deps) and dependency edges.
    fn expand_dependencies(
        &self,
        contracts: &[CapabilityContract],
    ) -> Result<(Vec<CapabilityContract>, Vec<DependencyEdge>), ResolverError> {
        let mut result_map: HashMap<CapabilityId, CapabilityContract> = HashMap::new();
        let mut dep_edges: Vec<DependencyEdge> = Vec::new();
        let mut queue: VecDeque<CapabilityId> = VecDeque::new();

        for c in contracts {
            let id = c.id.clone();
            result_map.insert(id.clone(), c.clone());
            queue.push_back(id);
        }

        let mut in_flight: HashSet<CapabilityId> = queue.iter().cloned().collect();

        while let Some(current_id) = queue.pop_front() {
            let current = result_map.get(&current_id)
                .ok_or_else(|| ResolverError::UnregisteredCapability(current_id.clone()))?;

            for dep_id in &current.dependencies {
                // Cycle detection: if dep is already in the queue (in_flight) but not yet fully
                // resolved (i.e. its own deps haven't been processed), that's a cycle.
                // Actually, for BFS expansion, a cycle would be: A -> B -> A.
                // Check if dep_id is already in result_map AND its deps are pending in queue.
                if in_flight.contains(dep_id) {
                    // Check if we'd create a cycle: dep_id depends on something that leads back
                    // For now, detect simple immediate cycles
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

                    result_map.insert(dep_id.clone(), dep_contract.clone());
                    queue.push_back(dep_id.clone());
                    in_flight.insert(dep_id.clone());
                }

                dep_edges.push(DependencyEdge {
                    from: current_id.clone(),
                    to: dep_id.clone(),
                });
            }
        }

        let mut result: Vec<CapabilityContract> = result_map.into_values().collect();
        // Deterministic ordering for cache stability
        result.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        Ok((result, dep_edges))
    }
```

Add the import at the top of the file:
```rust
use std::collections::VecDeque;
use super::graph::DependencyEdge;
```

- [ ] **Step 4: Integrate expansion into `resolve()`**

After the required/optional capability resolution (and version constraint resolution) and before graph construction, insert:

```rust
        // Collect all resolved contracts so far
        let resolved_contracts: Vec<CapabilityContract> = instances.iter().map(|i| i.contract.clone()).collect();

        // Expand transitive dependencies
        let (all_contracts, dep_edges) = self.expand_dependencies(&resolved_contracts)?;

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
        // Add dependency edges
        for edge in &dep_edges {
            graph.add_dependency(edge.from.clone(), edge.to.clone());
        }

        // Validate Graph Invariants (Dependencies, Conflicts, Cycles)
        graph.validate().map_err(|e| ResolverError::GraphValidationFailed(e))?;

        let resolved = ResolvedCapabilitySet { graph, instances: final_instances };
```

Remove the old graph construction, visited set, and validation. The full `resolve()` method becomes:

```rust
    pub fn resolve(&self, reqs: &RequirementSet) -> Result<ResolvedCapabilitySet, ResolverError> {
        if let Some(cached) = self.cache.get(reqs) {
            return Ok(cached);
        }

        let mut instances = Vec::new();
        let mut visited = HashSet::new();

        // Resolve version-constrained requirements
        for vc in &reqs.version_constraints {
            let contract = self.resolve_version(vc)?;
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

        // Resolve Optional Capabilities (Graceful Degradation)
        for opt_id in &reqs.optional_capabilities {
            let target_id = self.aliases.get(opt_id).unwrap_or(opt_id);
            if let Some(contract) = self.registry.get(target_id) {
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
        let (all_contracts, dep_edges) = self.expand_dependencies(&resolved_contracts)?;

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

        // Validate Graph Invariants (Dependencies, Conflicts, Cycles)
        graph.validate().map_err(|e| ResolverError::GraphValidationFailed(e))?;

        let resolved = ResolvedCapabilitySet { graph, instances: final_instances };
        self.cache.put(reqs.clone(), resolved.clone());
        Ok(resolved)
    }
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p fusion-router -- capability::resolver::tests`
Expected: 3 new tests pass, all existing tests pass

- [ ] **Step 6: Commit**

```bash
git add src/planner/resolver/capability/resolver.rs
git commit -m "feat(capability): add BFS dependency expansion to resolver"
```

---

### Task 5: Policy constraints

**Files:**
- Modify: `src/planner/resolver/capability/resolver.rs`

**Interfaces:**
- Consumes: `PolicyContext` (already defined in Task 3), `RequirementSet.policy`
- Produces: `apply_policy()` filtering method on `CapabilityResolver`

- [ ] **Step 1: Write failing tests for policy constraints**

```rust
    fn registry_with_tags() -> (Arc<dyn CapabilityRegistry>, Vec<CapabilityContract>) {
        let mut reg = InMemoryCapabilityRegistry::new();
        let stable = CapabilityContract {
            id: CapabilityId::new("stable.cap"),
            version: semver::Version::parse("1.0.0").unwrap(),
            description: "stable".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 10,
            reliability_score: 1.0,
            supports_streaming: false,
        };
        let experimental = CapabilityContract {
            id: CapabilityId::new("experimental.cap"),
            version: semver::Version::parse("0.9.0").unwrap(),
            description: "experimental".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 10,
            reliability_score: 0.5,
            supports_streaming: false,
        };
        reg.register(stable.clone()).unwrap();
        reg.register(experimental.clone()).unwrap();
        reg.freeze();
        (Arc::new(reg), vec![stable, experimental])
    }

    #[test]
    fn deny_list_excludes_capability() {
        let (registry, _) = registry_with_tags();
        let resolver = CapabilityResolver::new(registry);
        let reqs = RequirementSet {
            required_capabilities: vec![CapabilityId::new("stable.cap")],
            optional_capabilities: vec![],
            version_constraints: vec![],
            max_acceptable_latency_ms: None,
            max_cost_usd: None,
            policy: Some(PolicyContext {
                environment: "production".into(),
                allow_list: None,
                deny_list: vec![CapabilityId::new("stable.cap")],
                release_profile: None,
            }),
        };
        match resolver.resolve(&reqs) {
            Err(ResolverError::PolicyDenied { .. }) => {}
            other => panic!("expected PolicyDenied, got {other:?}"),
        }
    }

    #[test]
    fn allow_list_restricts_to_specific() {
        let (registry, _) = registry_with_tags();
        let resolver = CapabilityResolver::new(registry);
        let reqs = RequirementSet {
            required_capabilities: vec![CapabilityId::new("stable.cap")],
            optional_capabilities: vec![],
            version_constraints: vec![],
            max_acceptable_latency_ms: None,
            max_cost_usd: None,
            policy: Some(PolicyContext {
                environment: "production".into(),
                allow_list: Some(vec![CapabilityId::new("experimental.cap")]),
                deny_list: vec![],
                release_profile: None,
            }),
        };
        match resolver.resolve(&reqs) {
            Err(ResolverError::PolicyDenied { .. }) => {}
            other => panic!("expected PolicyDenied, got {other:?}"),
        }
    }

    #[test]
    fn policy_pass_allows_resolution() {
        let (registry, _) = registry_with_tags();
        let resolver = CapabilityResolver::new(registry);
        let reqs = RequirementSet {
            required_capabilities: vec![CapabilityId::new("stable.cap")],
            optional_capabilities: vec![],
            version_constraints: vec![],
            max_acceptable_latency_ms: None,
            max_cost_usd: None,
            policy: Some(PolicyContext {
                environment: "production".into(),
                allow_list: None,
                deny_list: vec![],
                release_profile: None,
            }),
        };
        let res = resolver.resolve(&reqs).unwrap();
        assert_eq!(res.instances.len(), 1);
        assert_eq!(res.instances[0].contract.id.as_str(), "stable.cap");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p fusion-router -- capability::resolver::tests`
Expected: Tests compile but fail because policy is not enforced yet

- [ ] **Step 3: Implement `apply_policy()` method**

```rust
    /// Filters a contract through policy constraints.
    /// Returns `Ok(())` if the contract passes all policy checks.
    fn apply_policy(&self, contract: &CapabilityContract, policy: &PolicyContext) -> Result<(), ResolverError> {
        // Deny list check
        if policy.deny_list.contains(&contract.id) {
            return Err(ResolverError::PolicyDenied {
                capability: contract.id.clone(),
                reason: "capability is in the deny list".into(),
            });
        }

        // Allow list check (if set, only explicitly allowed)
        if let Some(ref allow) = policy.allow_list {
            if !allow.contains(&contract.id) {
                return Err(ResolverError::PolicyDenied {
                    capability: contract.id.clone(),
                    reason: "capability is not in the allow list".into(),
                });
            }
        }

        Ok(())
    }
```

- [ ] **Step 4: Integrate policy into `resolve()`**

At the beginning of `resolve()`, before any resolution logic, add:

```rust
        // Apply policy constraints to requirements
        if let Some(ref policy) = reqs.policy {
            for req_id in &reqs.required_capabilities {
                let target_id = self.aliases.get(req_id).unwrap_or(req_id);
                if let Some(contract) = self.registry.get(target_id) {
                    self.apply_policy(contract, policy)?;
                }
            }
        }
```

This checks policy against registry contracts before attempting resolution. If a contract is denied, the error is caught early.

- [ ] **Step 5: Run tests**

Run: `cargo test -p fusion-router -- capability::resolver::tests`
Expected: 3 new policy tests pass, all previous tests pass (including semver and dependency tests)

- [ ] **Step 6: Commit**

```bash
git add src/planner/resolver/capability/resolver.rs
git commit -m "feat(capability): add policy constraint evaluation to resolver"
```

---

### Task 6: `CapabilityGraphLowerer`

**Files:**
- Create: `src/planner/resolver/capability/lowerer.rs`
- Modify: `src/planner/resolver/capability/mod.rs`

**Interfaces:**
- Consumes: `CapabilityGraph` (nodes, dependencies, topological sort)
- Produces: `ExecutionGraph` matching the compiler's struct in `src/types/mod.rs`
- Uses: `uuid::Uuid::new_v5` for deterministic ID mapping
- Requires: add `v5` feature to `uuid` dependency in `Cargo.toml`

- [ ] **Step 1: Write failing tests**

Add to `lowerer.rs` as `#[cfg(test)]`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use fusion_plugin_api::CapabilityContract;
    use serde_json::json;

    fn make_contract(id: &str) -> CapabilityContract {
        CapabilityContract {
            id: fusion_plugin_api::CapabilityId::new(id),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: format!("Test {}", id),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 1,
            reliability_score: 1.0,
            supports_streaming: false,
        }
    }

    #[test]
    fn lowering_produces_deterministic_output() {
        let mut graph = CapabilityGraph::new();
        graph.add_node(make_contract("cap.a"));
        graph.add_node(make_contract("cap.b"));
        graph.add_dependency(
            fusion_plugin_api::CapabilityId::new("cap.b"),
            fusion_plugin_api::CapabilityId::new("cap.a"),
        );

        let lowerer = CapabilityGraphLowerer;
        let eg1 = lowerer.lower(&graph);
        let eg2 = lowerer.lower(&graph);

        // Deterministic: same graph_id, same nodes, same edges
        assert_eq!(eg1.graph_id, eg2.graph_id);
        assert_eq!(eg1.nodes.len(), eg2.nodes.len());
        assert_eq!(eg1.nodes[0].id, eg2.nodes[0].id);
        assert_eq!(eg1.edges, eg2.edges);
    }

    #[test]
    fn lowering_preserves_topological_order() {
        let mut graph = CapabilityGraph::new();
        graph.add_node(make_contract("shell"));
        graph.add_node(make_contract("filesystem"));
        graph.add_node(make_contract("browser"));
        graph.add_dependency(
            fusion_plugin_api::CapabilityId::new("browser"),
            fusion_plugin_api::CapabilityId::new("filesystem"),
        );
        graph.add_dependency(
            fusion_plugin_api::CapabilityId::new("filesystem"),
            fusion_plugin_api::CapabilityId::new("shell"),
        );

        let lowerer = CapabilityGraphLowerer;
        let eg = lowerer.lower(&graph);

        // shell must come before filesystem, filesystem before browser
        let ids: Vec<_> = eg.nodes.iter().map(|n| n.config.get("capability_id").and_then(|v| v.as_str()).unwrap_or("")).collect();
        let shell_idx = ids.iter().position(|s| *s == "shell").unwrap();
        let fs_idx = ids.iter().position(|s| *s == "filesystem").unwrap();
        let browser_idx = ids.iter().position(|s| *s == "browser").unwrap();
        assert!(shell_idx < fs_idx);
        assert!(fs_idx < browser_idx);
    }

    #[test]
    fn empty_graph_lowers() {
        let graph = CapabilityGraph::new();
        let lowerer = CapabilityGraphLowerer;
        let eg = lowerer.lower(&graph);
        assert!(eg.nodes.is_empty());
        assert!(eg.edges.is_empty());
    }
}
```

- [ ] **Step 2: Add `v5` feature to uuid dependency**

In `Cargo.toml`, change:
```toml
uuid = { version = "1", features = ["v4", "serde"] }
```
To:
```toml
uuid = { version = "1", features = ["v4", "v5", "serde"] }
```

Required for `Uuid::new_v5()` deterministic ID generation.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p fusion-router -- capability::lowerer`
Expected: Compile error — `lowerer` module not found

- [ ] **Step 4: Create `src/planner/resolver/capability/lowerer.rs`**

Full file:

```rust
//! Deterministic lowering from `CapabilityGraph` to `ExecutionGraph`.
//!
//! This is a compiler transformation, not an intrinsic graph operation.
//! Keeping it as a separate type allows future optimization passes,
//! instrumentation insertion, and scheduling hints.

use std::collections::HashMap;
use uuid::Uuid;
use fusion_plugin_api::CapabilityId;
use crate::types::{
    ExecutionEdge, ExecutionGraph, ExecutionNode, ExecutionNodeKind,
    GraphMetadata, RetryPolicy, StrategyKind,
};
use super::graph::CapabilityGraph;

/// Lowering component: `CapabilityGraph` → `ExecutionGraph`.
///
/// Deterministic: identical input produces identical output.
pub struct CapabilityGraphLowerer;

impl CapabilityGraphLowerer {
    /// Lowers a `CapabilityGraph` into the compiler's `ExecutionGraph`.
    ///
    /// Each capability node becomes a `Gate` execution node.
    /// Dependency edges become execution edges preserving topological order.
    pub fn lower(&self, cap_graph: &CapabilityGraph) -> ExecutionGraph {
        // Deterministic topological ordering
        let order = match cap_graph.topological_sort() {
            Ok(order) => order,
            Err(_) => return ExecutionGraph {
                graph_id: Uuid::nil(),
                nodes: Vec::new(),
                edges: Vec::new(),
                metadata: GraphMetadata {
                    estimated_cost: 0.0,
                    estimated_tokens: 0,
                    max_depth: 0,
                    node_count: 0,
                },
                total_tokens: 0,
                total_cost: 0,
                primitive_graph_hash: 0,
            },
        };

        let mut id_map: HashMap<CapabilityId, Uuid> = HashMap::new();
        let mut nodes = Vec::new();
        let mut total_cost: u64 = 0;
        let mut total_tokens: u64 = 0;

        for cap_id in &order {
            let node_id = deterministic_uuid(cap_id);
            id_map.insert(cap_id.clone(), node_id);

            let node = cap_graph.get_node(cap_id).expect("node from topological sort must exist");
            total_cost += (node.contract.estimated_cost_usd * 1000.0) as u64;
            total_tokens += node.contract.estimated_latency_ms;

            let mut config = std::collections::HashMap::new();
            config.insert("capability_id".into(), serde_json::json!(cap_id.as_str()));
            config.insert("description".into(), serde_json::json!(node.contract.description));
            if !node.contract.permissions.is_empty() {
                config.insert("permissions".into(), serde_json::json!(node.contract.permissions));
            }

            nodes.push(ExecutionNode {
                id: node_id,
                kind: ExecutionNodeKind::Gate,
                strategy: StrategyKind::Single,
                model: String::new(),
                retry_policy: RetryPolicy {
                    max_retries: 2,
                    backoff_ms: 1000,
                },
                fallback: None,
                config,
            });
        }

        let mut edges = Vec::new();
        for dep in cap_graph.dependencies() {
            if let (Some(&from_id), Some(&to_id)) = (id_map.get(&dep.from), id_map.get(&dep.to)) {
                edges.push(ExecutionEdge {
                    from: from_id,
                    to: to_id,
                    condition: None,
                });
            }
        }

        ExecutionGraph {
            graph_id: deterministic_graph_uuid(&order),
            nodes,
            edges,
            metadata: GraphMetadata {
                estimated_cost: (total_cost as f64) / 1000.0,
                estimated_tokens: total_tokens,
                max_depth: cap_graph.node_count() as u32,
                node_count: cap_graph.node_count() as u32,
            },
            total_tokens,
            total_cost,
            primitive_graph_hash: 0,
        }
    }
}

/// Deterministic UUID from a `CapabilityId` string.
/// Uses UUID v5 with a fixed namespace so the same ID always produces the same UUID.
fn deterministic_uuid(cap_id: &CapabilityId) -> Uuid {
    const CAPABILITY_NAMESPACE: Uuid = Uuid::from_u128(0x6ba7b810_9dad_11d1_80b4_00c04fd430c8);
    Uuid::new_v5(&CAPABILITY_NAMESPACE, cap_id.as_str().as_bytes())
}

/// Deterministic graph UUID from an ordered list of capability IDs.
fn deterministic_graph_uuid(order: &[CapabilityId]) -> Uuid {
    const GRAPH_NAMESPACE: Uuid = Uuid::from_u128(0x6ba7b811_9dad_11d1_80b4_00c04fd430c8);
    let mut bytes = Vec::new();
    for id in order {
        bytes.extend_from_slice(id.as_str().as_bytes());
        bytes.push(0);
    }
    Uuid::new_v5(&GRAPH_NAMESPACE, &bytes)
}
```

- [ ] **Step 4: Update `mod.rs` to include lowerer module**

```rust
pub mod graph;
pub mod lowerer;
pub mod resolver;

pub use graph::{CapabilityGraph, CapabilityNode, DependencyEdge, ConflictEdge};
pub use lowerer::CapabilityGraphLowerer;
pub use resolver::{
    CapabilityResolver, CapabilityPlannerCache, RequirementSet,
    ResolvedCapabilitySet, ResolverError,
};
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p fusion-router -- capability::lowerer`
Expected: 3 tests pass (determinism, order, empty graph)

- [ ] **Step 6: Commit**

```bash
git add src/planner/resolver/capability/lowerer.rs src/planner/resolver/capability/mod.rs
git commit -m "feat(capability): add deterministic CapabilityGraphLowerer"
```

---

### Task 7: Integration verification

**Files:**
- Verify: whole workspace

- [ ] **Step 1: Full test suite**

Run: `cargo test`
Expected: All ~850+ tests pass

- [ ] **Step 2: Clippy check**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: Zero warnings

- [ ] **Step 3: No-default-features check**

Run: `cargo check --no-default-features --lib`
Expected: Compiles without optional runtime features

- [ ] **Step 4: Verify determinism invariant**

Run: `cargo test -p fusion-router -- lowering_produces_deterministic_output`
Expected: PASS

- [ ] **Step 5: Final log review**

Run: `git log --oneline -10`
Expected: Clean commit history

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "chore: Sprint O2.5 integration verification"
```
