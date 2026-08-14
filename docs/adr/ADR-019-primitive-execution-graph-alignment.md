# ADR-019: PrimitiveGraph / ExecutionGraph Alignment

- **Status**: Accepted
- **Date**: July 2026
- **Context**: FusionRouter v0.9 Phase 0 — Architectural Objective O-1
- **Deciders**: FusionRouter Engineering Team
- **Implementation Status**: Complete (v0.9 Phase 1)

---

## Context

After ADR-018 Phase 2, the execution pipeline contains two distinct graph representations:

1. **`PrimitiveGraph`** (`compiler::ir::primitive_ir`): A versioned DAG of scheduling primitives (FanOut, Barrier, Reducer, LLMGenerate, etc.). Produced by `Strategy::lower()`. Subject to optimization passes. Has deterministic hashing (`compute_hash()`), Mermaid/DOT export, and a serialization format suitable for golden IR snapshots.

2. **`ExecutionGraph`** (`types::ExecutionGraph`): A runtime DAG of `ExecutionNode` values with UUID-based node identity, `StrategyKind` annotations, model bindings, retry policies, and config maps. Consumed by `DefaultScheduler::schedule()` and `WorkQueue`. Produced by the compiler pipeline (`lower_to_graph()`) or, for strategy nodes, by the executor's `resolve_strategy()` path.

The current flow is:

```
StrategyIR
    │
    ▼  Strategy::lower()
PrimitiveGraph
    │
    ▼  executor::primitive_to_subgraph() (via resolve_strategy())
ExecutionSubgraph
    │
    ▼ executor::execute_node() — iterates subgraph nodes sequentially
NodeExecutionResult
```

Meanwhile the compiler pipeline produces `ExecutionGraph` directly:

```
WorkflowIR
    │
    ▼ compiler passes (lower_to_graph())
ExecutionGraph
    │
    ▼ scheduler::schedule()
ExecutionInstance
    │
    ▼ scheduler::run() — iterates ready nodes via WorkQueue
ExecutionResult
```

The architectural problem: **two graph representations, two production paths, no formal equivalence guarantee.** The `primitive_to_subgraph()` converter at `src/executor/mod.rs:315` bridges the gap within `resolve_strategy()`, but it is a runtime fallback — it clones `template.config` to every sub-node, skips FanOut/Barrier primitives, and produces an `ExecutionSubgraph` that bypasses the scheduler. The compiler's `ExecutionGraph` and the executor's `ExecutionSubgraph` are structurally independent.

### Concrete Symptoms

- `primitive_to_subgraph()` reroutes edges around Barrier nodes by scanning the edge list (`src/executor/mod.rs:386-398`). This logic is separate from the scheduler's `WorkQueue`, which also processes edges. Two independent edge-resolution algorithms must agree.
- `ExecutionSubgraph` has no `graph_id`, `metadata`, or provenance fields — it is a transient struct consumed immediately by `execute_node()`. No record of which `PrimitiveGraph` produced it.
- Optimizers in the compiler pass pipeline transform `WorkflowIR`, not `PrimitiveGraph`. The `optimization` module (`src/compiler/optimization/`) defines `OptimizationPass` for `PrimitiveGraph`, but no pass has been implemented because there is no pipeline that connects optimized `PrimitiveGraph` back to the scheduler.

---

## Decision Drivers

| Driver | Description |
|--------|-------------|
| **Single source of truth** | One graph representation should be authoritative. The other should be derived or eliminated. |
| **Deterministic compilation** | Identical input + identical lowering must produce identical execution. Currently `primitive_to_subgraph()` generates new UUIDs per call (`Uuid::new_v4()` at line 332), breaking determinism. |
| **Runtime simplicity** | The scheduler should not need to understand two graph formats. Fewer representations means fewer code paths. |
| **Replay / provenance** | Every `ExecutionResult` must reference the exact `PrimitiveGraph` that produced it. The graph hash (`PrimitiveGraph::compute_hash()`) is the identity key. |
| **Backwards compatibility** | Existing plugin `Strategy::apply()` implementations, golden IR snapshots, and integration tests should not break during migration. |
| **Migration effort** | The cost of changing the scheduler, executor, compiler passes, and tests. |
| **Performance** | The scheduler loop must not regress. Additional conversion or validation passes add latency. |
| **Plugin / API stability** | External strategy plugins should not need to understand `PrimitiveGraph` internals unless they implement a lowering pass. |

---

## Options

### Option A — Mechanical Derivation

`PrimitiveGraph` remains the canonical representation. `ExecutionGraph` is eliminated as an independently-mutable type and becomes a generated runtime artifact — produced in a single pass from `PrimitiveGraph` with no separate modifiability.

**How it works:**

1. `Strategy::lower()` produces `PrimitiveGraph`. This is the only place strategy topology is defined.
2. Optimization passes operate on `PrimitiveGraph` only.
3. A new function `execution_graph_from_primitive(pg: &PrimitiveGraph) -> ExecutionGraph` replaces `primitive_to_subgraph()`. It is a pure, deterministic conversion: given the same `PrimitiveGraph`, it always produces the same `ExecutionGraph`. The UUID assigned to each node is derived from the graph hash plus the node index (e.g., `Uuid::from_u128(graph_hash as u128 ^ node_index as u128)`), not random.
4. The scheduler consumes `ExecutionGraph` as before. No scheduler changes needed.
5. `ExecutionGraph` is never modified after creation. Any transformation goes through `PrimitiveGraph` first, then re-derivation.

**Impact on existing code:**
- `primitive_to_subgraph()` is replaced (not removed — the `ExecutionSubgraph` type may still be useful for inline strategy expansion, but its UUID generation becomes deterministic).
- `resolve_strategy()` becomes: `lower() → optimize() → execution_graph_from_primitive()`. The fallback to `apply()` is removed (Phase 1 of the roadmap).
- `PrimitiveGraph` gains a `to_execution_graph()` method.
- `ExecutionGraph` gains a `from_primitive_graph` provenance field: `primitive_graph_hash: u64`.

### Option B — Strict Invariant Enforcement

Both `PrimitiveGraph` and `ExecutionGraph` coexist, but a compiler pass validates structural equivalence before execution.

**How it works:**

1. `Strategy::lower()` produces `PrimitiveGraph`. The compiler also produces `ExecutionGraph` via the existing `lower_to_graph()` path.
2. A new validation pass `EquivalenceValidationPass` compares the two graphs:
   - Node count and kind correspondence.
   - Edge topology isomorphism.
   - Model binding consistency.
3. If validation fails, execution is aborted with a `CompilerDiagnostic`. No silent drift.
4. Golden IR snapshots include both representations.

**Impact on existing code:**
- `EquivalenceValidationPass` is added to the compiler pass list.
- `primitive_to_subgraph()` and `lower_to_graph()` both remain. Both must be kept in sync — any change to one representation requires a corresponding change to the other.
- The equivalence check runs on every request, adding O(n) validation overhead.
- Provenance records both graph hashes.

### Option C — Merge: Eliminate `ExecutionGraph`

The scheduler consumes `PrimitiveGraph` directly. `ExecutionGraph` is removed entirely.

**How it works:**

1. `DefaultScheduler::schedule()` accepts `PrimitiveGraph` instead of `ExecutionGraph`.
2. `WorkQueue` operates on `PrimitiveNode` and `PrimitiveEdge` instead of `ExecutionNode` and `ExecutionEdge`.
3. `ExecutionNode` (with its UUIDs, model bindings, retry policies, config maps) is flattened into `PrimitiveNode` or carried as an annotation layer.
4. The compiler's `lower_to_graph()` is replaced by a direct `PrimitiveGraph` producer.

**Impact on existing code:**
- `ExecutionGraph`, `ExecutionNode`, `ExecutionEdge` types may be removed or reduced to pure annotations.
- `DefaultScheduler`, `WorkQueue`, `ExecutionInstance` must be rewritten to operate on `PrimitiveGraph`.
- The entire integration test suite that constructs `ExecutionGraph` directly must be updated.
- All golden IR snapshots (which are `PrimitiveGraph`-based) become the sole reference.
- Plugin `Strategy::apply()` implementations (which return `ExecutionSubgraph`) become incompatible.

---

## Trade-off Matrix

| Criterion | Option A (Derivation) | Option B (Validation) | Option C (Merge) |
|-----------|----------------------|----------------------|------------------|
| **Implementation complexity** | Medium — new conversion fn, deterministic UUID, remove fallback | Low — new validation pass only | High — rewrite scheduler, WorkQueue, types |
| **Architectural clarity** | High — single source of truth | Medium — two graphs, one invariant | Highest — one graph |
| **Migration risk** | Low — scheduler unchanged, existing tests pass | Low — additive, no existing code changes | High — every test that creates ExecutionGraph breaks |
| **Runtime performance** | Negligible — deterministic UUID math is O(n) | O(n) validation per request | Slight improvement — one less conversion |
| **Testability** | High — pure function, deterministic output | Medium — equivalence pass must handle false positives | High — one graph to test |
| **Provenance** | Single graph hash | Two graph hashes, must reconcile | Single graph hash |
| **Future optimization** | Optimize PrimitiveGraph, re-derive | Must sync both graphs after optimization | Optimize the only graph |
| **Plugin compatibility** | `apply()` fallback removed (planned) | `apply()` can remain | `apply()` must be removed or return PrimitiveGraph |
| **Backwards compatibility** | UUID-based node identity changes (deterministic) | No behavioral change | Breaking — all execution graph APIs change |
| **OSS contributor onboarding** | One canonical conversion to understand | Two graphs + validation pass | One graph to understand |

---

## Recommendation

**Option A — Mechanical derivation.**

The decision is driven by three factors:

1. **Architectural trajectory.** ADR-018 already establishes `PrimitiveGraph` as the IR that lowering produces and optimizers transform. ADR-017 establishes `ExecutionResult` as the runtime ABI. Option A completes the migration: `PrimitiveGraph` becomes the sole graph representation between lowering and execution, with `ExecutionGraph` as a pure derived artifact. This is the natural end state of the compiler-first architecture.

2. **Implementation cost vs. benefit.** The scheduler does not need to change. `WorkQueue` continues to consume `ExecutionGraph`. The migration is localized to the executor (`resolve_strategy()`, `primitive_to_subgraph()`) and a new `PrimitiveGraph::to_execution_graph()` method. No integration test that constructs `ExecutionGraph` directly breaks — they continue to produce `ExecutionGraph` as before; the only change is that production code derives it deterministically from `PrimitiveGraph`.

3. **Determinism.** The existing `primitive_to_subgraph()` generates random UUIDs (line 332 of `src/executor/mod.rs`: `let uid = Uuid::new_v4()`), which makes every invocation produce a different `ExecutionSubgraph` even for identical `PrimitiveGraph` input. Deterministic derivation fixes this trivially and unblocks graph-level caching, golden IR replay, and provenance.

### Concrete Decision

- `PrimitiveGraph` gains a method:
  ```rust
  impl PrimitiveGraph {
      /// Deterministically produce an ExecutionGraph from this PrimitiveGraph.
      /// Node UUIDs are derived from (graph_hash, node_index) so identical
      /// PrimitiveGraphs always produce identical ExecutionGraphs.
      pub fn to_execution_graph(&self) -> ExecutionGraph { ... }
  }
  ```
- `primitive_to_subgraph()` is replaced by `to_execution_graph()`. The `ExecutionSubgraph` type is retained for inline strategy execution but its UUID generation becomes deterministic.
- `resolve_strategy()` uses `to_execution_graph()` instead of `primitive_to_subgraph()`. The fallback to `Strategy::apply()` is removed (per roadmap Phase 1 timeline).
- `ExecutionGraph` gains a provenance field:
  ```rust
  pub struct ExecutionGraph {
      // ... existing fields ...
      pub primitive_graph_hash: u64,  // from PrimitiveGraph::compute_hash()
  }
  ```
- All optimizers operate on `PrimitiveGraph` only.

---

## Migration Plan

Mapped to Phase 1 tasks in `docs/roadmap-v0.9.md`:

### Step 1 — Add `to_execution_graph()` to `PrimitiveGraph` (0.5 day)

Implement the deterministic conversion. Node UUIDs are `Uuid::from_u128(graph_hash as u128 ^ node_index as u128)`. The FanOut and Barrier primitives are skipped (as in the current `primitive_to_subgraph()`), but the edge-rerouting logic is formalized: edges from a skippable node are expanded to all immediate successors. Model bindings and config maps are carried from the PrimitiveNode's `role` field and artifact annotations.

### Step 2 — Add `primitive_graph_hash` to `ExecutionGraph` (0.5 day)

Field addition to the struct. Populated by `to_execution_graph()`. Serialized in golden IR snapshots for provenance tracking.

### Step 3 — Replace `primitive_to_subgraph()` usage (0.5 day)

Update `resolve_strategy()` in `src/executor/mod.rs` to call `to_execution_graph()` and then `scheduler.schedule()` instead of returning an `ExecutionSubgraph` for inline execution. This removes the dual-path execution model entirely — the scheduler is the only execution engine.

### Step 4 — Remove `Strategy::apply()` fallback (0.5 day)

Delete the `apply()` method from the `Strategy` trait. Remove the fallback branch in `resolve_strategy()`. Update all strategy implementations.

### Step 5 — Update golden IR snapshots (1 day)

Regenerate golden snapshots to include `primitive_graph_hash`. Verify that all existing golden tests pass with deterministic UUIDs.

### Step 6 — Update examples (0.5 day)

Update `examples/debate_roadmap.rs` and `examples/consensus_roadmap.rs` to use `to_execution_graph()` and print the `primitive_graph_hash` from the resulting `ExecutionGraph`.

---

## Related Documents

- ADR-003: Compiler — establishes the compiler pass pipeline
- ADR-017: Execution Runtime ABI — establishes `ExecutionResult` as the runtime contract
- ADR-018: Strategy SDK — establishes `PrimitiveGraph`, `Strategy::lower()`, and the dual-path transition
- `docs/roadmap-v0.9.md` — Phase 0 Foundation, Architectural Objective O-1
- `src/executor/mod.rs:315` — `primitive_to_subgraph()` converter
- `src/compiler/ir/primitive_ir.rs` — `PrimitiveGraph` definition
- `src/types/mod.rs:154` — `ExecutionGraph` definition
- `src/scheduler/default.rs` — scheduler dependency on `ExecutionGraph`

---

## Unresolved Questions

1. **Should `PrimitiveNode` carry the `model` string directly, or should model binding remain in `ExecutionNode`?** Currently the model is embedded in `PrimitiveNodeKind::LLMGenerate { model }`. Option A preserves this. A follow-up decision may move model binding to a separate resolution layer.

2. **Should `to_execution_graph()` handle all `PrimitiveNodeKind` variants or only the subset that the scheduler understands?** The scheduler does not handle `FanOut` or `Barrier` — those are expanded during conversion. Other variants (e.g., `ConditionalBranch`, `FeedbackLoop`) have direct `ExecutionNodeKind` equivalents. The conversion should be exhaustive: unrecognized variants produce a `CompilerDiagnostic`.
