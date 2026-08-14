# Governance Architecture Specification: AF-006

## Specification Title: AF-006 Distributed Runtime Consistency & 4th Core Engine

**Architecture Version:** AF-006  
**Status:** Frozen  
**Target Release:** FusionRouter v0.14.3 Distributed Runtime Foundation  

---

## 1. Executive Summary

AF-006 formalizes the elevation of **Placement** into FusionRouter's **4th Core Engine**, establishing crisp boundaries across the execution lifecycle:

| Engine | Responsibility | Input Artifact | Output Artifact |
|---|---|---|---|
| **1. Planner** | What should be done? | Natural Language Prompt | `WorkflowIR` |
| **2. Compiler** | How is it represented? | `WorkflowIR` | `ExecutionGraph`, `CompilerReport` |
| **3. Placement Engine** | Where should each node execute? | `ExecutionGraph` | `PlacementGraph`, `PlacementReport` |
| **4. Scheduler** | When should nodes execute? | `PlacementGraph` | `ExecutionPlan` |
| **5. Runtime** | Execute and observe | `ExecutionPlan` | `ExecutionContext`, `RuntimeCheckpoint` |

---

## 2. Frozen Governance Contracts (AF-006)

1. **Placement Contract v1**: Decides worker assignments without mutating `ExecutionGraph` topology.
2. **PlacementGraph v1**: Immutable placed execution graph recording exact node-to-worker assignments.
3. **PlacementReport v1**: Detailed score breakdown (`execution_id`, `graph_hash`, `placement_policy`, `node_decisions`, `rejected_workers`, `score_breakdown`).
4. **Cluster State Contract v1**: Single authoritative worker registry (`ClusterNodeInfo`) combining static `WorkerCapabilities` and dynamic `WorkerStatus`.
5. **Execution Lease Contract v1**: Explicit time-bounded lease mechanism governing task execution and failover under **Invariant 13 (Single-Worker Lease Exclusivity)**.
6. **Worker Capabilities Contract v1**: Standardized `WorkerCapabilities` schema (`llm_models`, `memory_mb`, `has_gpu`, `tools`, `max_parallelism`, `locality_zone`, `labels`, `protocol_version`).
7. **ExecutionPlan Contract v1**: Immutable canonical `ExecutionPlan` output from Scheduler (`plan_id`, `placement_id`, `execution_order`, `worker_assignments`).
8. **PlacementId Lineage v1**: Strongly-typed `PlacementId` linking `ExecutionId` → `WorkflowId` → `GraphId` → `PlacementId` → `ExecutionPlanId`.
9. **Pluggable Placement Policy Rule**: `PlacementPolicy` implementations are 100% pluggable and replaceable without altering Placement Contract v1 or runtime interfaces.
