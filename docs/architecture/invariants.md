# Architectural Invariants (AF-003)

This document specifies the immutable architectural invariants for FusionRouter v0.14.0 and beyond.

---

### Invariant 1: Immutable WorkflowIR
`WorkflowIR` is immutable after construction. No pass or subsystem may mutate a `WorkflowIR` instance once created; optimizations transform `WorkflowIR` into new optimized instances or lower them into `ExecutionGraph` instances.

### Invariant 2: Immutable ExecutionGraph
`ExecutionGraph` is immutable after compilation. Execution state (node progress, results, retries) is maintained in `ExecutionContext` rather than mutating the underlying graph DAG topology.

### Invariant 3: Deterministic Compiler Passes
Compiler passes are 100% deterministic. Given the identical input `WorkflowIR` and compiler configuration, compilation must produce an identical `ExecutionGraph`.

### Invariant 4: Isolated Planner
The Planner never invokes external providers or tools directly. The Planner's sole responsibility is resolving intent into a `WorkflowIR` DAG over the `CapabilitySystem`.

### Invariant 5: Worker Boundary
Workers execute `Execution ABI v1` tasks assigned by the Coordinator. Workers never perform workflow planning or DAG optimization.

### Invariant 6: Decoupled Studio Storage
Fusion Studio UI never accesses database repositories directly. All Studio operations flow through the public REST (`/api/v1/*`) or WebSocket (`/ws/events`) APIs in `fusion-studio-api`.

### Invariant 7: Pure Storage Repositories
Storage repositories in `fusion-infrastructure` handle persistence and transaction safety exclusively. Repositories never contain domain business logic.

### Invariant 8: Kernel Independence
`fusion-kernel` has zero infrastructure, storage, network, or UI dependencies. Kernel interfaces communicate solely through versioned contracts.

### Invariant 9: Versioned Public Contracts
Every externally consumed public contract (`REST API`, `Worker Protocol`, `Plugin SDK`, `WorkflowIR`, `Execution ABI`) must carry an explicit version tag (`v1`).

### Invariant 10: Strongly-Typed Execution IDs
Every execution is assigned a unique, strongly-typed `ExecutionId` at creation time, which correlates all logs, metrics, telemetry, and evidence.

### Invariant 11: Versioned Performance Contracts
Performance contracts (SLOs for Planner <10ms, Compiler <20ms, Scheduler <5ms, Runtime Overhead <10ms, Replay <20ms) are versioned alongside API contracts and enforced via regression testing.

### Invariant 12: Single Source of Truth (Migration Law)
Every subsystem has exactly one authoritative implementation located in its designated workspace crate (`crates/fusion-*`). Compatibility modules in `src/` may re-export workspace symbols but must never duplicate business logic. No parallel implementations, mirrored logic, or temporary copies are permitted.

### Invariant 13: Single-Worker Lease Exclusivity
Every `ExecutionGraph` node is leased by at most one worker at any instant. Workers execute tasks under explicit, time-bounded leases issued by the Placement Engine; expired or revoked leases revert to the Coordinator for crash recovery.

### Invariant 14: Immutable PlacementGraph and ExecutionPlan
`PlacementGraph` and `ExecutionPlan` are immutable after construction. Neither Placement Engine nor Scheduler mutates past execution plan graphs; retries or failovers generate new versioned plan instances under a new `ExecutionPlanId`.

### Invariant 15: Deterministic Placement Engine
Given identical `PlacementPolicy`, `ClusterState`, and `ExecutionGraph`, the Placement Engine produces an identical `PlacementGraph` and `PlacementReport`. Placement decisions are 100% deterministic to guarantee side-effect-free offline replay simulation.
