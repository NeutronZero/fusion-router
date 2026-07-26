# Provenance Schema for ExecutionResult

- **Date**: July 2026
- **Status**: Proposed

## Context

Every `ExecutionResult` must carry enough provenance to uniquely identify *how* it was produced — which `PrimitiveGraph`, which compiler passes, which strategy, and which graph version. This enables golden IR replay, audit trails, and debugging across optimization boundaries.

Current `ExecutionResult` (`src/types/mod.rs:244`) has **no provenance fields**:

```rust
pub struct ExecutionResult {
    pub instance_id: Uuid,
    pub success: bool,
    pub outputs: HashMap<Uuid, serde_json::Value>,
    pub total_latency_ms: u64,
    pub total_cost: f64,
    pub total_tokens: u64,
    pub terminal_node_id: Option<Uuid>,
    pub final_output: Option<serde_json::Value>,
}
```

Provenance data is already produced upstream but discarded before `ExecutionResult` is constructed:

| Source | Field | Available at |
|--------|-------|-------------|
| `PrimitiveGraph::compute_hash()` | `graph_hash: u64` | `ExecutionGraph.primitive_graph_hash` (`src/types/mod.rs:162`) |
| `PRIMITIVE_GRAPH_VERSION` | `version: u16` | `src/compiler/ir/primitive_ir.rs:13` |
| `OptimizationPipeline` passes | `pass_manifest: Vec<String>` | ADR-020, not yet wired |
| `StrategyKind` | `strategy: StrategyKind` | `ExecutionNode.strategy` (`src/types/mod.rs:169`) |

## Schema

Every `ExecutionResult` SHOULD carry these four provenance fields:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    /// Deterministic hash of the PrimitiveGraph that produced this result.
    /// Computed via PrimitiveGraph::compute_hash() on the *optimized* graph
    /// (i.e., after all optimization passes, before to_execution_graph()).
    pub graph_hash: u64,

    /// Schema version of the PrimitiveGraph IR (PRIMITIVE_GRAPH_VERSION).
    /// Bumped when the PrimitiveGraph struct changes in a breaking way.
    pub primitive_graph_version: u16,

    /// Ordered list of optimization passes applied.
    /// Empty if the optimization pipeline was disabled.
    /// Each entry is the pass name (e.g. "dead_node_elimination").
    pub pass_manifest: Vec<String>,

    /// The strategy that produced this execution.
    /// Derived from ExecutionNode::strategy; all nodes in a graph
    /// share the same strategy after lowering.
    pub strategy: StrategyKind,
}
```

### Integration into `ExecutionResult`

**Option A — Inline** (recommended for early adoption):

```rust
pub struct ExecutionResult {
    pub instance_id: Uuid,
    // ... existing fields ...
    /// Provenance metadata describing how this result was produced.
    pub provenance: Provenance,
}
```

**Option B — Sidecar** (if serialization size is a concern):

```rust
pub struct ExecutionResult {
    pub instance_id: Uuid,
    // ... existing fields ...
    pub provenance_hash: u64,  // hash of Provenance struct stored in audit log
}
```

The `Provenance` struct itself can be stored in the `AuditEntry.details` JSON field (`src/telemetry/audit.rs:11`).

## Data flow

```
PrimitiveGraph (pre-opt)            → graph_hash (intermediate, not final)
    │
    ▼ OptimizationPipeline
PassManifest ← [dead_node_elim, fanout_consol, ...]
    │
    ▼ PrimitiveGraph (post-opt)
graph_hash ← PrimitiveGraph::compute_hash()
version    ← PRIMITIVE_GRAPH_VERSION
strategy   ← from PrimitiveGraph's lowering context
    │
    ▼ PrimitiveGraph::to_execution_graph()
ExecutionGraph.primitive_graph_hash = graph_hash
    │
    ▼ Scheduler + Executor
ExecutionResult.provenance = Provenance { graph_hash, version, pass_manifest, strategy }
```

## Audit log correlation

The `AuditEntry.details` field SHOULD contain the `Provenance` struct for every execution record:

```json
{
  "timestamp": 1750000000,
  "request_id": "abc-123",
  "action": "execute",
  "result": "success",
  "details": {
    "provenance": {
      "graph_hash": 18446744073709551615,
      "primitive_graph_version": 1,
      "pass_manifest": ["dead_node_elimination", "fanout_consolidation"],
      "strategy": "Debate"
    }
  }
}
```

This allows replaying any execution by re-building the `PrimitiveGraph` and verifying `compute_hash()` matches.
