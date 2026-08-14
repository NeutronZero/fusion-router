# Architectural Invariants (AF-003)

This document specifies the immutable architectural invariants for FusionRouter v0.14.0 and beyond.

---

### Invariant 1: Immutable WorkflowIR
`WorkflowIR` is immutable after construction. No pass or subsystem may mutate a `WorkflowIR` instance once created; optimizations transform `WorkflowIR` into new optimized instances or lower them into `ExecutionGraph` instances.

### Invariant 2: Immutable ExecutionGraph
`ExecutionGraph` is immutable after compilation. Execution state (node progress, results, retries) is maintained in `ExecutionContext` rather than mutating the underlying graph DAG topology.

### Invariant 3: Deterministic Compiler Passes
Compiler passes are 100% deterministic. Given identical input `WorkflowIR` and compiler configuration, compilation must produce byte-identical canonical `ExecutionGraph` output.

### Invariant 4: Isolated Planner
The Planner never invokes external providers or tools directly. The Planner is a snapshot-driven deterministic planner with catalog selection: it resolves intent into a `WorkflowIR` DAG over control-plane capability, model, policy, and telemetry snapshots. It does not claim adaptive runtime routing.

### Invariant 5: Worker Boundary
Workers execute `Execution ABI v1` tasks assigned by the Coordinator. Workers never perform workflow planning or DAG optimization.

### Invariant 6: Pure Storage Repositories
Storage repositories in `fusion-infrastructure` handle persistence and transaction safety exclusively. Repositories never contain domain business logic.

### Invariant 7: Kernel Independence
`fusion-kernel` has zero infrastructure, storage, network, or UI dependencies. Kernel interfaces communicate solely through versioned contracts.

### Invariant 8: Versioned Public Contracts
Every externally consumed public contract (`REST API`, `Worker Protocol`, `Plugin SDK`, `WorkflowIR`, `Execution ABI`) must carry an explicit version tag (`v1`).

### Invariant 9: Strongly-Typed Execution IDs
Every execution is assigned a unique, strongly-typed `ExecutionId` at creation time, which correlates all logs, metrics, telemetry, and evidence.

### Invariant 10: Versioned Performance Contracts
Performance contracts (SLOs for Planner <10ms, Compiler <20ms, Scheduler <5ms, Runtime Overhead <10ms, Replay <20ms) are versioned alongside API contracts and enforced via regression testing.

### Invariant 11: Single Source of Truth (Migration Law)
Every subsystem has exactly one authoritative implementation located in its designated workspace crate (`crates/fusion-*`). Compatibility modules in `src/` may re-export workspace symbols but must never duplicate business logic. No parallel implementations, mirrored logic, or temporary copies are permitted.

### Invariant 12: Single-Worker Lease Exclusivity
Every `ExecutionGraph` node is leased by at most one worker at any instant. Workers execute tasks under explicit, time-bounded leases issued by the Placement Engine; expired or revoked leases revert to the Coordinator for crash recovery.

### Invariant 13: Immutable PlacementGraph and ExecutionPlan
`PlacementGraph` and `ExecutionPlan` are immutable after construction. Neither Placement Engine nor Scheduler mutates past execution plan graphs; retries or failovers generate new versioned plan instances under a new `ExecutionPlanId`.

### Invariant 14: Deterministic Placement Engine
Given identical `PlacementPolicy`, `ClusterState`, and `ExecutionGraph`, the Placement Engine produces an identical `PlacementGraph` and `PlacementReport`. Placement decisions are 100% deterministic to guarantee side-effect-free offline replay.

### Invariant 15: Semantic Adapter Annotation
The execution ABI may use a smaller operational node-kind vocabulary than the planning IR. When lowering collapses planning kinds, the adapter must preserve the original planning meaning in the explicit `semantic_kind` annotation field; this is metadata preservation, not native typed execution-kind preservation.

### Invariant 16: Canonical Monetary Accounting
All internal monetary accounting and pricing rates use `NanoUSD`. Decimal USD values are accepted only at configuration or external presentation boundaries and are converted before runtime accounting.

### Invariant 17: Control-Plane Authority
The running application has one `PolicyRegistry` and one frozen `CapabilityRegistry`. `AppState`, operations, planning, and runtime consumers receive those same instances rather than constructing parallel registries.
