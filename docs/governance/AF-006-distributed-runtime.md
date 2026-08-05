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
| **4. Scheduler** | When should nodes execute? | `PlacementGraph` | Execution Timeline |
| **5. Runtime** | Execute and observe | Execution Timeline | `ExecutionContext`, `RuntimeCheckpoint` |

---

## 2. Frozen Governance Contracts (AF-006)

1. **Placement Contract v1**: Decides worker assignments without mutating `ExecutionGraph` topology.
2. **PlacementGraph v1**: Immutable placed execution graph recording exact node-to-worker assignments.
3. **PlacementReport v1**: Detailed score breakdown (`execution_id`, `graph_hash`, `placement_policy`, `node_decisions`, `rejected_workers`, `score_breakdown`).
4. **Cluster State Contract v1**: Single authoritative worker registry (`ClusterNodeInfo`) shared across Scheduler, Placement Engine, and Studio.
5. **Execution Lease Contract v1**: Explicit time-bounded lease mechanism governing task execution and failover under **Invariant 13 (Single-Worker Lease Exclusivity)**.
