# ADR-021: Capability Platform Architecture & Immutable Registry

- **Status**: Proposed
- **Date**: July 2026
- **Context**: FusionRouter v0.10.0 Architecture Specification
- **Deciders**: FusionRouter Core Architecture Team

---

## Context

In FusionRouter v0.8.0 and v0.9.0, tools and integrations (e.g. file reads, shell commands, web requests) were coupled directly into engine execution paths. As FusionRouter expands, hardcoding integrations creates maintenance bloat and limits third-party extension.

v0.10.0 transforms FusionRouter into a **Capability Platform** where all external capabilities, tools, reasoning strategies, security policies, and connectors become pluggable components.

---

## Decisions

### 1. Unified Execution Flow

The platform separates discovery, resolution, planning, compilation, scheduling, and connector binding into explicit compiler-centric stages:

```text
Plugin Manager
       │
       ▼
Capability Registry (immutable)
       │
       ▼
Capability Resolver (Symbol Resolution & Dependency Checks)
       │
       ▼
Planner (Intent & Workflow IR Generation)
       │
       ▼
PrimitiveGraph (IR)
       │
       ▼
Policy Compiler Pass (Auto-inserts Approval / Policy Nodes)
       │
       ▼
Optimization Passes (Dead Node Elimination, Consolidation)
       │
       ▼
ExecutionGraph (Lowered IR)
       │
       ▼
Scheduler / Runtime
       │
       ▼
Connector Resolver (Late Binding of Capability to Connector)
       │
       ▼
Plugin Executors (Execution Phase)
```

### 2. Immutable `CapabilityRegistry` at Runtime

`PluginManager` discovers and validates plugins during initialization. Once startup completes, `PluginManager` constructs the `CapabilityRegistry` and **freezes** it. 

During planning and runtime execution, `CapabilityRegistry` is strictly read-only (`Arc<CapabilityRegistry>`).

#### Benefits
- **Determinism**: The capability graph cannot mutate during graph compilation.
- **Reproducible Planning**: Guaranteed graph hash stability across identical requests.
- **Thread-Safety**: Zero lock contention during high-throughput query planning.

---

## Consequences

- All tools and system capabilities must be registered prior to registry freeze.
- Dynamic runtime plugin hot-reloading requires spawning a new frozen snapshot of the registry rather than mutating the active registry in place.
