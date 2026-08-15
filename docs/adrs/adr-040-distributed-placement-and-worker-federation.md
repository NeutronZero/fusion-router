# ADR-040: Distributed Placement Engine and Worker Federation

- **Status:** Accepted
- **Date:** 2026-08-15
- **Applies to:** `crates/fusion-placement`, `crates/fusion-worker`, `crates/fusion-worker-protocol`, `crates/fusion-scheduler`
- **Depends on:** ADR-033 (Architecture Freeze), ADR-034 (Single Compiler Pipeline)

## Context

FusionRouter v0.14.5 established a fully converged, compiler-driven single-node execution engine where `WorkflowIR` compiles deterministically into an immutable `ExecutionGraph`.

As the system scales towards multi-node and heterogeneous compute clusters (Roadmap v0.15), execution requirements evolve:
1. Workload stages may require specialized hardware (e.g., local GPU nodes vs. cloud API workers).
2. Large agentic subgraphs should execute with network locality and load-aware distribution.
3. Node execution must remain fail-closed, with single-worker exclusivity and monotonic epoch heartbeats to prevent split-brain state or duplicated spend.

Rather than modifying the frozen compiler pipeline (`fusion-compiler`, `fusion-planner`, `fusion-scheduler`), the distributed execution architecture introduces a dedicated **Placement Engine** and **Execution Lease Manager** in `crates/fusion-placement`.

## Decision

### 1. Separation of Compiler and Placement Engine
The compiler pipeline remains authoritative, pure, and single-node agnostic:
- `WorkflowIR` $\rightarrow$ `ExecutionGraph` is unchanged (100% deterministic, zero entropy).
- Post-compilation, the `PlacementEngine` consumes `(ExecutionGraph, ClusterState, PlacementPolicy)` to produce an immutable `PlacementGraph` and `PlacementReport`.

### 2. Multi-Dimensional Placement Scoring Formula
Placement evaluates candidate workers using a deterministic multi-vector score:

$$\text{Placement Score} = (\text{Capability} \times 0.30) + (\text{Locality} \times 0.25) + (\text{Load} \times 0.20) + (\text{Latency} \times 0.15) + (\text{Cost} \times 0.10)$$

Where:
- **Capability Score (30%)**: Matches node capability tags (e.g. `GPU`, `128kContext`, `ToolExecution`) against worker advertised capabilities.
- **Locality Score (25%)**: Prioritizes workers co-located in the same region/zone as upstream data or tools.
- **Load Score (20%)**: Inverse function of worker CPU utilization and active task count.
- **Latency Score (15%)**: Real-time probe and heartbeat latency.
- **Cost Score (10%)**: Economic cost rate of the compute node.

A decision is marked optimal when $\text{Total Score} \ge 0.80$.

### 3. Invariant 12: Single-Worker Lease Exclusivity
Every node in the execution graph is leased to at most one worker at any instant:
```rust
pub struct ExecutionLease {
    pub lease_key: String,
    pub execution_id: String,
    pub node_id: String,
    pub worker_id: String,
    pub epoch: u64,
    pub granted_at_ms: u64,
    pub ttl_ms: u64,
    pub is_revoked: bool,
}
```
- **Exclusive Grant**: `grant_lease()` rejects lease acquisition attempts by worker $B$ if an active unexpired lease exists for worker $A$.
- **Monotonic Epochs**: Heartbeats and lease renewals monotonically advance `epoch`. Out-of-order renewals are rejected.
- **Crash Recovery**: Expired or explicitly revoked leases revert to the coordinator scheduler for failover under a incremented epoch.

### 4. Deterministic Placement Identification
Placement IDs are derived via canonical hashing of `(execution_id, node_count, policy_name)`, ensuring offline replay simulation (`Invariant 15`) produces identical placement decisions without side-effects.

## Consequences

- **No Compiler Pollution**: Compiler passes remain pure and single-process testable.
- **Explicit Distributed Contracts**: `ExecutionPlan` and `PlacementGraph` provide structured auditability before tasks are dispatched to workers.
- **Crash Safety**: Time-bounded leases ensure worker node drops do not stall DAG scheduling.
- **Zero Orphaned Tasks**: Worker tasks execute under explicit capability bounds and TTLs.
