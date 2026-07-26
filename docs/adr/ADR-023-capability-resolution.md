# ADR-023: Capability Resolution Subsystem, CapabilityGraph & CapabilityInstance

- **Status**: Proposed
- **Date**: July 2026
- **Context**: FusionRouter v0.10.0 Capability Resolution
- **Deciders**: FusionRouter Core Architecture Team

---

## Context

In complex agent workflows, capabilities have inter-dependencies (e.g. `Browser` capability depends on `Filesystem` capability, which depends on `Shell` capability). Blending capability lookup directly into LLM planning logic causes tight coupling and fragile plan generation.

---

## Decisions

### 1. Dedicated Capability Resolver Subsystem

Capability resolution is treated as compiler **symbol resolution**. It lives in a dedicated module (`src/planner/resolver/capability/`).

#### Pipeline
```text
Planner (Intent Extraction)
       │
       ▼
Requirement Set
       │
       ▼
Capability Resolver ◄─── CapabilityGraph & CapabilityPlannerCache
       │
       ▼
Resolved Capability Set
       │
       ▼
CapabilityInstance (Bound Runtime Execution Object)
       │
       ▼
PrimitiveGraph Generation
```

### 2. `CapabilityContract` as Semantic ABI

`CapabilityContract` is defined as the formal **semantic ABI** between the Planner and the Scheduler. It encapsulates input/output JSON schemas, permissions, side effects, latency, cost, and streaming guarantees.

### 3. `CapabilityInstance` Abstraction

`CapabilityInstance` represents the bound runtime realization of a `CapabilityContract`. Analogous to a compiled function pointer or bound execution handle, it pairs the abstract `CapabilityContract` with runtime execution parameters (e.g., account contexts, scope limits) resolved prior to graph execution.

### 4. CapabilityGraph (Dependency DAG)

The system constructs a `CapabilityGraph` tracking capability dependencies, conflict declarations, and version constraints.

```text
Capabilities:
  GitHub
    └── HTTP
         └── OAuth

  Browser
    └── Filesystem
         └── Shell
```

The resolver verifies that all capability dependencies are satisfied, resolves conflicts, and checks version compatibility before graph construction begins.

### 5. Capability Planner Cache

To prevent repeated expensive resolution queries when processing frequent requirement sets, the resolver utilizes an LRU `CapabilityPlannerCache` mapping `RequirementSet` hash → `ResolvedCapabilitySet`.

---

## Consequences

- Prevents invalid graph generation when required underlying capabilities are missing.
- Provides a clean `CapabilityInstance` abstraction for bound execution contexts without polluting the Planner or Scheduler.
- Significantly accelerates planning throughput via caching.
