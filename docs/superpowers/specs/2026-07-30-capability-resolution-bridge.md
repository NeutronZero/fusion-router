# Sprint O2.5 — Capability Resolution Bridge

- **Status:** Draft
- **Date:** 2026-07-30
- **Subsystem:** Capability Platform / Resolution Layer

---

## 1. Context

Sprint O1 established the developer SDK and the `#[capability]` macro. Sprint O2 introduced the typed `Permission` ABI, the `CapabilityRegistry` trait, and `InMemoryCapabilityRegistry` — completing the Capability Discovery Layer.

The v0.12 architecture requires a **Resolution Layer** that bridges discovery (O2) and the compiled `ExecutionGraph` runtime.

The `src/planner/resolver/capability/` module already contains:
- `CapabilityGraph` — DAG with dependency/conflict edges, cycle detection via Kahn's algorithm, topological sort
- `CapabilityResolver` — exact-ID resolution with aliases, latency filtering, `CapabilityPlannerCache`
- `RequirementSet` — required + optional capability IDs with cost/latency bounds
- `ResolvedCapabilitySet` — output bundle of graph + instances

These components are mature and tested. They do not need to be rewritten.

---

## 2. Goals

1. Extend `CapabilityResolver` with semantic version resolution (caret, tilde, exact, latest).
2. Add transitive dependency expansion to the resolver (resolved before graph construction).
3. Introduce policy constraint evaluation in the resolver (environment, allow/deny lists, release profiles).
4. Add deterministic lowering from `CapabilityGraph` to `ExecutionGraph`.
5. Preserve all existing invariants: alias resolution, caching, freeze semantics, cycle detection, conflict detection, topological ordering.

---

## 3. Non-Goals

- Rewriting `CapabilityGraph`, `CapabilityResolver`, or cycle/conflict detection.
- Changing `CapabilityRegistry` or its freeze semantics.
- Introducing a second-generation resolver.
- Modifying the scheduler or executor.
- Adding runtime policy enforcement (policy is a selection concern only).
- Changing existing `ExecutionGraph` or `ExecutionNode` types.

---

## 4. Architectural Changes

| Component | Current State | Sprint O2.5 Change |
|-----------|--------------|--------------------|
| `CapabilityContract` (ABI) | No dependency declaration | Add `dependencies: Vec<CapabilityId>` |
| `RequirementSet` | `Vec<CapabilityId>` only | Add optional version constraints, policy tags |
| `CapabilityResolver` | Exact-ID resolution | Add semver resolution, dependency expansion, policy evaluation |
| `CapabilityPlannerCache` | LRU by `RequirementSet` | No change (cache key evolves with version constraints) |
| `CapabilityGraph` | DAG, cycles, conflicts | Preserved as-is |
| `ResolvedCapabilitySet` | graph + instances | Add resolution metadata |
| New: `CapabilityGraphLowerer` | Doesn't exist | `CapabilityGraph → ExecutionGraph` |
| `ExecutionGraph` | Compiler's runtime graph | No structural change |

---

## 5. Work Item A — SemVer Resolution

### Current behavior

```text
Requirement("echo.text")
    ↓
Registry.get("echo.text")     // exact ID match only
    ↓
Contract(id="echo.text", version=0.1.0)
```

### Target behavior

```text
Requirement(name="echo.text", version_req="^1.2")
    ↓
Registry.list() → filter by name prefix match
    ↓
Semver match (caret, tilde, exact, wildcard)
    ↓
Best compatible contract
```

### `RequirementSet` changes

```rust
pub struct VersionConstraint {
    pub capability_prefix: String,       // e.g. "echo"
    pub requirement: semver::VersionReq, // e.g. "^1.2"
}
```

Add to `RequirementSet`:
```rust
pub struct RequirementSet {
    pub required_capabilities: Vec<CapabilityId>,     // unchanged
    pub optional_capabilities: Vec<CapabilityId>,     // unchanged
    pub version_constraints: Vec<VersionConstraint>,  // NEW
    pub max_acceptable_latency_ms: Option<u64>,        // unchanged
    pub max_cost_usd: Option<u64>,                     // unchanged
}
```

### Resolution logic

Requirements are either **named** (exact `CapabilityId`) or **version-constrained** (prefix + semver range). They are independent — a single requirement is expressed as one or the other, not both.

1. Named requirements (`required_capabilities` entries without a matching prefix in `version_constraints`): resolved by exact-ID lookup (current behavior).
2. Version-constrained requirements (entries in `version_constraints`): query registry via `list()`, filter contracts where `id.as_str().starts_with(prefix)`, apply `semver::VersionReq.matches()`, select highest compatible version.
3. On conflict (multiple contracts match the same requirement): error.

### Error behavior

- `ResolverError::NoCompatibleVersion { capability, requirement }` if no contract satisfies the version constraint.
- `ResolverError::AmbiguousResolution { capability, matches: Vec<CapabilityId> }` if multiple contracts match without tiebreaker.

---

## 6. Work Item B — Dependency Expansion

### Design decision: dependency declaration on `CapabilityContract`

Add to `fusion-plugin-api`:

```rust
pub struct CapabilityContract {
    // ... existing fields ...
    pub dependencies: Vec<CapabilityId>,  // NEW
}
```

This makes dependencies a first-class ABI concern. Plugins declare what they depend on — the resolver expands them.

### Expansion algorithm

```text
resolve(reqs: RequirementSet) -> CapabilityGraph:
    1. resolve_versions(reqs)       → Vec<CapabilityContract>
    2. expand_dependencies(contracts) → Vec<CapabilityContract>
    3. build_graph(all_contracts)    → CapabilityGraph
    4. validate(graph)               → Result<(), String>
```

`expand_dependencies` is a BFS:

```
queue = initial_contracts
visited = {id for id in initial_contracts}

while queue not empty:
    contract = queue.pop_front()
    for dep_id in contract.dependencies:
        if dep_id not in visited:
            dep_contract = registry.get(dep_id)
            if dep_contract:
                visited.insert(dep_id)
                queue.push_back(dep_contract)
                graph.add_dependency(contract.id, dep_id)
```

### Cycle detection

Expansion happens *before* graph construction. Cycles are still detected by `CapabilityGraph::validate()` during graph construction. The expander can also detect a cycle during BFS (if a dependency is already in the queue but not yet resolved) and reject early.

### Error behavior

- `ResolverError::UnresolvedDependency { capability, dependency }` if a declared dependency is not found in the registry.
- `ResolverError::CircularDependency` if a cycle is detected during expansion.

---

## 7. Work Item C — Policy Constraints

### Philosophy

The registry is passive — it stores contracts. The resolver applies policy. Policy is a selection concern, not a storage concern.

### Constraint types (Phase 1)

```rust
pub struct PolicyContext {
    pub environment: String,                          // "production", "staging", "development"
    pub allow_list: Option<Vec<CapabilityId>>,        // if set, only these are allowed
    pub deny_list: Vec<CapabilityId>,                 // always excluded
    pub release_profile: Option<String>,              // "stable", "prerelease", "experimental"
}
```

### Integration with RequirementSet

```rust
pub struct RequirementSet {
    // ... existing fields + version_constraints ...
    pub policy: Option<PolicyContext>,   // NEW
}
```

### Evaluation order

```
resolve(reqs):
    1. policy check           → filter out denied, enforce allow-list
    2. resolve versions       → filter by semver
    3. expand dependencies    → BFS across allowed contracts
    4. build graph            → CapabilityGraph
    5. validate graph
```

### Policy granularity

Capabilities can declare metadata that interacts with policies (via `tags` and `categories` on `CapabilityDescriptor`):
- Tags like `"production-ready"`, `"experimental"` — matched against release profile.
- Categories used for group-level allow/deny.

The resolver evaluates:
1. **Deny list** — remove any contract whose ID is in the deny list.
2. **Allow list** — if set, only contracts in the allow list are eligible.
3. **Release profile** — if `"stable"`, filter to contracts with tag `"production-ready"`.
4. **Environment** — future use for environment-scoped capability visibility.

### Error behavior

- `ResolverError::PolicyDenied { capability, reason }` if a capability fails policy evaluation.

---

## 8. Work Item D — CapabilityGraph → ExecutionGraph Lowering

### Rationale for a dedicated lowerer

Lowering is a compiler transformation, not an intrinsic graph operation. A dedicated `CapabilityGraphLowerer` provides space for future optimization passes, instrumentation insertion, and scheduling hints without growing `CapabilityGraph`.

### Types

```rust
pub struct CapabilityGraphLowerer;

impl CapabilityGraphLowerer {
    pub fn lower(&self, cap_graph: &CapabilityGraph) -> ExecutionGraph;
}
```

### Lowering rules

Each `CapabilityNode` in the `CapabilityGraph` maps to an `ExecutionNode`:

| CapabilityGraph concept | ExecutionGraph mapping |
|------------------------|----------------------|
| `CapabilityNode` | `ExecutionNode` |
| `DependencyEdge` | `ExecutionEdge` (direction preserved) |
| Topological order | `ExecutionGraph.nodes` order |
| Node metadata | `ExecutionNode.config` + `ExecutionNode.model` |
| `DependencyEdge.from→to` | `ExecutionEdge.from→to` |

```rust
fn lower(cap_graph: &CapabilityGraph) -> ExecutionGraph {
    let order = cap_graph.topological_sort()?;  // deterministic
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Node mapping
    for cap_id in &order {
        let node = cap_graph.get_node(cap_id).unwrap();
        nodes.push(ExecutionNode {
            id: deterministic_id(cap_id),
            kind: ExecutionNodeKind::Gate,   // capabilities are Gates by default
            strategy: StrategyKind::Single,
            model: String::new(),
            retry_policy: default_retry(),
            fallback: None,
            config: config_from_contract(&node.contract),
        });
    }

    // Edge mapping (dependency order → execution order)
    for dep in cap_graph.dependencies() {
        edges.push(ExecutionEdge {
            from: id_map[&dep.from],
            to: id_map[&dep.to],
            condition: None,
        });
    }

    ExecutionGraph { ... }
}
```

### Determinism guarantee

Given identical `CapabilityGraph` input, two invocations of `lower()` produce byte-identical `ExecutionGraph` output. This relies on:
- `topological_sort()` returns a deterministic order (Kahn's algorithm with deterministic tie-breaking by `CapabilityId` lexical order).
- `deterministic_id()` is a pure function of the `CapabilityId` string (uses UUID v5 with a fixed namespace, or a hash-based mapping — must be stable across runs).
- No external state (randomness, timestamps) influences the mapping.

### Placement

The lowerer lives in `src/planner/resolver/capability/lowerer.rs` — co-located with the graph and resolver, since it transforms a capability concept into the compiler's runtime concept.

---

## 9. Resolver Error Enum

The current resolver returns `Result<ResolvedCapabilitySet, String>`. O2.5 migrates this to a typed error:

```rust
pub fn resolve(&self, reqs: &RequirementSet) -> Result<ResolvedCapabilitySet, ResolverError>;
```

This is backwards-compatible: `ResolverError` implements `Display` + `std::error::Error`, so existing callers that match on `String` can migrate incrementally.

```rust
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
```

---

## 10. Testing

### SemVer resolution
- Exact version match
- Caret (`^1.2`) resolves to highest compatible
- Tilde (`~2.0`) resolves to highest patch
- No compatible version → error
- Ambiguous resolution without tiebreaker → error

### Dependency expansion
- Single-level expansion
- Multi-level (transitive) expansion
- Missing dependency → error
- Circular dependency detected during BFS
- Duplicate dependency (same dep declared by two capabilities) → no error, single inclusion

### Policy constraints
- Deny list excludes capability
- Allow list restricts to specific capabilities
- Release profile filters by tag
- All policies pass → normal resolution

### Lowering determinism
- Two calls to `lower(&graph)` produce identical `ExecutionGraph`
- Topological order preserved in output
- Edges correctly mapped
- Node metadata populated from contract fields

### Regression
- All existing CapabilityGraph tests pass (cycle, conflict, topological sort)
- All existing CapabilityResolver tests pass (resolution, alias, cache)
- All existing registry tests pass

---

## 11. Success Criteria

1. Existing `CapabilityResolver` extended with semver resolution.
2. Resolver expands transitive capability dependencies (BFS before graph construction).
3. Resolver applies policy constraints (allow/deny lists, release profile) before selection.
4. Existing `CapabilityGraph` reused as the planning graph — no structural changes.
5. Deterministic lowering from `CapabilityGraph` to `ExecutionGraph` via `CapabilityGraphLowerer`.
6. Existing cycle/conflict detection, alias resolution, and caching preserved.
7. **Resolver determinism:** identical registry + requirements + policy → identical `CapabilityGraph` and `ExecutionGraph`.
8. No regression in existing test suite (345+ tests).

---

## 12. Architectural Invariants

| Invariant | Scope | Enforced By |
|-----------|-------|-------------|
| Registry is passive | All sprints | Only `register()`, `get()`, `list()` — no resolution logic |
| Resolver never mutates registry | O2, O2.5 | `&self` on resolver, `Arc<dyn CapabilityRegistry>` |
| Graph is acyclic post-validation | O2, O2.5 | `CapabilityGraph::validate()` → topological sort |
| Freeze prevents registration | O2 | Checked in `InMemoryCapabilityRegistry::register()` |
| Lowering is deterministic | O2.5 | `topological_sort()` + pure ID mapping |
| Policy is selection-only | O2.5 | Evaluated before graph construction, not in scheduler |
| Dependencies expand transitively | O2.5 | BFS in `expand_dependencies()` |
| CapabilityContract declares dependencies | O2.5 | New `dependencies: Vec<CapabilityId>` field |
