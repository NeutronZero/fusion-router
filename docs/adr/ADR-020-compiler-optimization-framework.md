# ADR-020: Compiler Optimization Framework

- **Status**: Accepted
- **Date**: July 2026
- **Context**: FusionRouter v0.9 Phase 0 — Optimization pass selection criteria
- **Deciders**: FusionRouter Engineering Team

---

## Context

After ADR-019, the compiler pipeline has a clear architectural flow:

```
WorkflowIR
    │
    ▼ CompilerPass pipeline (validation, resolution, lowering)
PrimitiveGraph
    │
    ▼ OptimizationPass pipeline (future — this ADR)
PrimitiveGraph (optimized)
    │
    ▼ PrimitiveGraph::to_execution_graph()
ExecutionGraph
    │
    ▼ DefaultScheduler::schedule()
ExecutionInstance
```

The `OptimizationPass` trait and `OptimizationPipeline` exist at `src/compiler/optimization/mod.rs` but have never been used — no pass has been implemented. The roadmap calls for Phase 2 ("Optimize — v0.9.0-beta") with at least two concrete optimizations: dead node elimination and FanOut consolidation.

Before implementing any optimization pass, the project needs a formal framework that defines:

1. **What qualifies as a compiler optimization** — which transformations belong in the optimization pipeline vs. other pipeline stages.
2. **What every optimization must guarantee** — legality conditions, invariants that cannot be violated.
3. **How optimizations are selected** — admission criteria that prevent speculative or unmeasurable passes.
4. **How optimizations compose** — ordering rules, conflict resolution, and rollback semantics.

This ADR establishes that framework. It is the architectural contract for all optimization work in v0.9 and beyond.

---

## Decision Drivers

| Driver | Description |
|--------|-------------|
| **Determinism** | Every optimization must preserve deterministic compilation. Identical input + identical pass sequence = identical output. |
| **Semantic preservation** | Optimizations may change graph structure but must not change observable execution behavior (node semantics, dataflow edges, strategy intent). |
| **Measurable benefit** | Every optimization must declare a measurable objective and be rejected if it cannot demonstrate benefit. |
| **Composability** | Optimizations must compose safely — the output of one pass must be a valid input to the next. |
| **Testability** | Every optimization must be independently testable via golden IR snapshots. |
| **Rollback safety** | Failed optimizations must never leave the graph in a partially-transformed state. |
| **Provenance transparency** | Optimized graphs must carry sufficient metadata for debugging and replay — what passes ran, in what order, with what parameters. |

---

## Compiler Pass Taxonomy

The compiler pipeline already has a `CompilerPass` trait (`src/compiler/passes/mod.rs`) that operates on `WorkflowIR`. The optimization pipeline operates on `PrimitiveGraph`. These are distinct domains with different contracts.

The full pass taxonomy is:

### Validation Pass

Operates on: `WorkflowIR` (pre-lowering)

**Purpose:** Reject malformed or unsupported IR before it enters the pipeline.

**Guarantees:**
- Pure — no graph transformation
- Fails fast on invariant violation
- Produces `CompilerDiagnostic` with actionable error messages

**Examples:** Node kind validation, edge connectivity checks, budget constraint validation.

**Existing:** The compiler's pass pipeline includes validation-like passes (e.g., `ConstraintValidation` per ADR-003).

### Lowering Pass

Operates on: `WorkflowIR` → `PrimitiveGraph`

**Purpose:** Convert high-level IR into the primitive representation. This is where strategy selection, model resolution, and structural decomposition happen.

**Guarantees:**
- Produces structurally valid `PrimitiveGraph`
- Not subject to rollback — lowering either succeeds or fails
- Single-pass: no iterative refinement

**Existing:** `Strategy::lower()` produces `PrimitiveGraph` directly. The compiler's `lower_to_graph()` converts `WorkflowIR` → `ExecutionGraph` (legacy path, to be consolidated).

### Analysis Pass

Operates on: `PrimitiveGraph` (read-only)

**Purpose:** Compute properties of the graph without modifying it. Analysis results feed into optimization decisions.

**Guarantees:**
- Read-only — no graph mutation
- Deterministic — same graph always produces same analysis
- Output is a typed analysis result (not a diagnostic)

**Examples:** Dead node detection, edge density analysis, critical path length, FanOut reachability.

### Optimization Pass

Operates on: `PrimitiveGraph` → `PrimitiveGraph`

**Purpose:** Transform the graph to improve a measurable objective while preserving semantics.

**Guarantees:**
- Semantic preservation (see Legality Rules below)
- Deterministic transformation
- Full rollback on failure
- No side effects

**Examples:** Dead node elimination, FanOut consolidation, constant folding, barrier elision.

### Instrumentation Pass

Operates on: `PrimitiveGraph` → `PrimitiveGraph`

**Purpose:** Annotate nodes or edges with metadata for observability, debugging, or profiling. Does not change execution behavior.

**Guarantees:**
- Execution-identical — removing instrumentation produces identical execution
- No semantic impact
- Reversible

**Examples:** Node label injection, provenance tagging, execution count annotations.

### Verification Pass

Operates on: `PrimitiveGraph` (read-only)

**Purpose:** Verify that post-optimization invariants hold. Runs after optimizations but before `to_execution_graph()`.

**Guarantees:**
- Read-only
- Produces `CompilerDiagnostic` on violation
- Can be skipped in production (configurable)

**Examples:** Graph hash consistency check, edge connectivity verification, scheduler compatibility validation.

---

## Optimization Goals

Every optimization must declare its objective from the following taxonomy. A pass may serve multiple goals but must declare a **primary** goal.

| Goal | Definition | Measurement |
|------|------------|-------------|
| **Latency** | Reduce end-to-end execution time | Wall-clock time per execution |
| **Token cost** | Reduce total LLM token consumption | Input + output tokens per execution |
| **Memory** | Reduce peak memory usage during scheduling | Peak RSS or heap allocation |
| **Graph simplification** | Reduce node/edge count without semantic change | Node count delta, edge count delta |
| **Provider utilization** | Improve load balancing or reduce provider contention | Provider call distribution, queue depth |
| **Determinism** | Remove sources of non-determinism | Variance across repeated executions |

### Prohibited Goals

An optimization pass must not declare any of the following as its objective:

- **Readability** — graph readability is a debugging concern, not an optimization objective
- **Backwards compatibility** — compatibility is preserved by the framework, not by individual passes
- **Feature enablement** — enabling new functionality is the domain of lowering or instrumentation passes

---

## Legality Rules

Every optimization pass must preserve the following invariants. Violation of any rule is a compiler bug.

### Rule 1: Semantic Preservation

The observable behavior of every `PrimitiveNode` must be identical before and after optimization.

- `LLMGenerate` nodes must remain `LLMGenerate` with the same `model` and `role`. The pass may not change the model identifier, inject new LLM calls, or remove LLM calls.
- `Reducer` nodes must remain `Reducer` with the same `mode` and `model`. Aggregation semantics are invariant.
- `ConditionalBranch` conditions must remain logically equivalent. The pass may not invert, merge, or split conditions.
- `FeedbackLoop` iteration limits must not be reduced below the original value. They may be increased (tighter bound) only if analysis proves the loop never exceeds the lower bound.

Exception: `FanOut` and `Barrier` nodes may be inserted, removed, or consolidated because they are scheduling primitives with no observable execution semantics — they only control parallelism.

### Rule 2: Determinism Preservation

Given the same `PrimitiveGraph` input, an optimization pass must produce the same output graph every time.

- Random number generation is prohibited within optimization passes.
- Time-based or environment-dependent logic is prohibited.
- Hash-based decisions must use `PrimitiveGraph::compute_hash()` as the seed.

### Rule 3: Provenance Preservation

- The optimized graph must carry a provenance record of which passes were applied.
- The final `primitive_graph_hash` in `ExecutionGraph` must reflect the optimized graph, not the pre-optimization graph.
- Each pass should record its identity in an optimization manifest attached to the graph.

### Rule 4: Graph Hash Invariant

- After optimization, `PrimitiveGraph::compute_hash()` returns a new value that reflects the transformed graph.
- The hash change is expected and correct — it signals that the graph was modified.
- Verification passes can compare the pre- and post-optimization hash but must not require them to match.
- Any pass that recomputes node IDs must re-derive them deterministically from the new graph hash.

### Rule 5: Scheduler Compatibility

The optimized graph must produce a valid `ExecutionGraph` via `PrimitiveGraph::to_execution_graph()`.

- No dangling edges (edges referencing non-existent node IDs).
- No unreachable nodes that were previously reachable (dead nodes may be removed, but reachable nodes must retain at least one incoming path from a root).
- All node kinds must be convertible to `ExecutionNodeKind` (i.e., no `PrimitiveNodeKind` variant that the scheduler does not handle).

---

## Pass Contract

The existing `OptimizationPass` trait is:

```rust
pub trait OptimizationPass: Send + Sync {
    fn name(&self) -> &str;
    fn optimize(&self, graph: PrimitiveGraph) -> Result<PrimitiveGraph, CompilerDiagnostic>;
}
```

This ADR extends the contract with documentation requirements and verification hooks:

```rust
pub trait OptimizationPass: Send + Sync {
    /// Human-readable pass name (e.g., "dead_node_elimination")
    fn name(&self) -> &str;

    /// Primary optimization goal from the taxonomy
    fn goal(&self) -> OptimizationGoal;

    /// Preconditions that must hold for this pass to be safe.
    /// Returns an error if preconditions are not met.
    fn preconditions(&self, graph: &PrimitiveGraph) -> Result<(), CompilerDiagnostic>;

    /// Transform the graph.
    fn optimize(&self, graph: PrimitiveGraph) -> Result<PrimitiveGraph, CompilerDiagnostic>;

    /// Verify that post-optimization invariants hold.
    fn postconditions(&self, original: &PrimitiveGraph, optimized: &PrimitiveGraph) -> Result<(), CompilerDiagnostic>;
}

pub enum OptimizationGoal {
    Latency,
    TokenCost,
    Memory,
    GraphSimplification,
    ProviderUtilization,
    Determinism,
}
```

The `preconditions` and `postconditions` methods are **verification hooks**, not runtime checks. They exist so that:

1. **Tests** can call `preconditions()` and `postconditions()` directly without running the full pipeline.
2. **Golden IR snapshots** can verify that every optimization in a sequence satisfied its contract.
3. **The verification pipeline** can run postcondition checks in debug mode or CI without runtime overhead in production.

The `OptimizationPipeline::run()` method should call `preconditions()` before each pass and `postconditions()` after, but only when `cfg(debug_assertions)` is enabled or when explicitly configured for verification mode.

---

## Selection Criteria

An optimization pass is accepted into the pipeline only if it satisfies **all** of the following criteria:

### Criterion 1: Measurable Benefit

The pass must declare a measurable objective and a benchmark that demonstrates improvement.

- **Acceptable:** "Dead node elimination reduces node count by 5-15% on typical workflow graphs, reducing scheduler overhead proportionally. Benchmark: `benchmarks/dead_node_elimination.rs` shows 8.3% mean reduction across 20 standard workflows."
- **Unacceptable:** "This pass makes graphs cleaner." (subjective)
- **Unacceptable:** "This pass might improve latency." (speculative)

### Criterion 2: Deterministic Transformation

The pass must produce identical output for identical input. The test suite must include:

- A test that runs the pass twice on the same input and asserts `==`.
- A test that runs the pass on a randomly generated graph and asserts no panic or invariant violation.

### Criterion 3: Replay-Safe

The pass must not depend on external state (time, RNG, network, filesystem). The output must be fully determined by the input `PrimitiveGraph`.

### Criterion 4: Independently Testable

The pass must have:

- A unit test for each distinct transformation case (e.g., "eliminates node with no consumers", "preserves node with consumers").
- At least one golden IR test showing input → expected output.
- A test for the degenerate case (empty graph, single node, fully connected graph).

### Criterion 5: Golden IR Coverage

Every transformation the pass performs must be representable as a golden IR pair (input `PrimitiveGraph` JSON → expected output `PrimitiveGraph` JSON). These snapshots serve as regression tests and documentation.

### Criterion 6: Complexity Proportional to Benefit

The pass must be simple enough that its maintenance cost does not exceed its benefit.

- **Acceptable:** A 50-line pass that saves 10% token cost.
- **Questionable:** A 500-line pass that saves 0.5% latency on specific topologies.
- **Unacceptable:** A 1000-line pass with no demonstrated benefit.

### Criterion 7: Named and Documented

The pass must have:

- A descriptive name (used by `OptimizationPass::name()`).
- A doc comment explaining what it does, when it applies, and what it guarantees.
- An entry in the optimization registry (a new `OptimizationRegistry` similar to `StrategyRegistry`).

---

## Ordering

Optimization order matters because:

1. **Dead node elimination should run first.** It reduces the graph size, making downstream passes cheaper. It also removes nodes that downstream passes might otherwise waste time analyzing.
2. **FanOut consolidation should run second.** It restructures parallelism boundaries. This is safer after dead nodes are removed because a dead node might have been the sole consumer of a FanOut, making the FanOut itself eliminable.
3. **Analysis passes can run between optimization passes** to provide fresh data. For example, dead node analysis runs before dead node elimination; edge density analysis could run before FanOut consolidation.
4. **Verification runs last.** After all optimizations complete, a verification pass confirms that invariants hold.

### Default Order

```
PrimitiveGraph (input)
    │
    ▼ Analysis: dead node detection
    │
    ▼ Dead Node Elimination
    │
    ▼ Analysis: edge density / FanOut reachability
    │
    ▼ FanOut Consolidation
    │
    ▼ Verification: scheduler compatibility, connectivity
    │
    ▼ PrimitiveGraph (output → to_execution_graph())
```

### Composing New Passes

When a new pass is added, its author must specify:

1. **Position** — where in the sequence it belongs (e.g., "after Dead Node Elimination, before FanOut Consolidation").
2. **Dependencies** — which analysis results it requires (e.g., "requires live node set").
3. **Provided analyses** — which analysis results it makes available for downstream passes (e.g., "provides updated live node set").

These are documented in the pass's doc comment, not encoded in the type system. The pass author is responsible for verifying that the pass works correctly at its declared position.

### Conflict Resolution

If two passes interact in unexpected ways (e.g., Pass A removes a node that Pass B expected to exist), the resolution is:

1. **Change the order** — reorder passes so that Pass B runs before Pass A.
2. **Add preconditions** — Pass B declares a precondition that fails if the expected node is absent, causing a rollback.
3. **Remove the conflict** — if the passes cannot coexist, the less valuable pass is removed.

---

## Rollback

The compiler already has transactional rollback semantics (ADR-003, `test_transactional_rollback`). Every pass in the pipeline clones the IR before invoking the pass:

```rust
match pass.apply(current.clone()).await {
    Ok(next) => current = next,
    Err(e) => return Err(e),  // current is still the pre-pass snapshot
}
```

The optimization pipeline follows the same pattern. The existing `OptimizationPipeline::run()` does this:

```rust
for pass in &self.passes {
    graph = pass.optimize(graph)?;  // ownership transfer: on error, graph is consumed
}
```

This is **not** rollback-safe — if `optimize()` fails, the original graph is lost. The correct pattern is:

```rust
pub fn run(&self, graph: PrimitiveGraph) -> Result<PrimitiveGraph, CompilerDiagnostic> {
    let mut current = graph;
    for pass in &self.passes {
        let snapshot = current.clone();
        match pass.optimize(current) {
            Ok(next) => current = next,
            Err(e) => return Err(e),  // snapshot is dropped — OK for error case
        }
    }
    Ok(current)
}
```

Wait — the above still has a problem: on error, we have the `snapshot` but `current` was consumed. The correct pattern:

```rust
pub fn run(&self, graph: PrimitiveGraph) -> Result<PrimitiveGraph, CompilerDiagnostic> {
    let mut current = graph;
    for pass in &self.passes {
        let snapshot = current.clone();
        current = match pass.optimize(current) {
            Ok(next) => next,
            Err(e) => {
                // Pass failed; snapshot is available in debug builds for analysis,
                // but the pipeline aborts — return the error, not the snapshot.
                return Err(e);
            }
        };
    }
    Ok(current)
}
```

The clone cost is O(n) in graph size. For typical workflow graphs (< 100 nodes), this is negligible. For pathological cases, the optimization pipeline could run in a separate memory space or use copy-on-write, but that is future work.

### Partial Transformation

A pass must not partially transform the graph. If an optimization detects midway through that it cannot complete, it must return an error without having modified the graph.

**Constraint:** The pass receives `PrimitiveGraph` by value (ownership). It must either return `Ok(transformed)` or `Err(diagnostic)` — there is no intermediate state. The compiler enforces this at the type level: the caller owns the graph, and the pass either returns a new graph or an error. No shared mutable state.

### Recovery

When an optimization pass fails, the pipeline does not retry. The error propagates to the caller, which can:

1. **Fall back to the unoptimized graph** — execute without optimization.
2. **Abort execution** — the optimization failure indicates a compiler invariant violation that should not happen silently.
3. **Re-route to a degraded pipeline** — skip the failing pass and retry remaining passes.

The default behavior (v0.9) is **option 1**: fall back to the unoptimized `PrimitiveGraph`. This ensures that optimization failures never block execution. The fallback is logged at `WARN` level.

---

## Implementation Plan

### Step 1 — Update `OptimizationPipeline` for rollback safety (0.5 day)

Change `OptimizationPipeline::run()` to use the snapshot pattern described in the Rollback section. Add a configuration flag for verification mode (enables `preconditions()` and `postconditions()` calls).

### Step 2 — Extend `OptimizationPass` trait (0.5 day)

Add `goal()`, `preconditions()`, and `postconditions()` as described in the Pass Contract. Default implementations return `Ok(())` for pre/post conditions so existing (nonexistent) passes don't break.

### Step 3 — Implement Dead Node Elimination (2 days)

**Scope:**
- An analysis pass that computes the set of live nodes (nodes reachable from any root via edge traversal).
- An optimization pass that removes non-live nodes and their edges.

**Selection criteria coverage:**
- Measurable benefit: node count reduction on real workflow graphs.
- Deterministic: pure function of graph topology.
- Golden IR tests: 3+ snapshots (no dead nodes, some dead nodes, all nodes dead).
- Complexity: ~100 lines.

### Step 4 — Implement FanOut Consolidation (2 days)

**Scope:**
- Adjacent FanOut nodes are merged into a single FanOut with `count = max(count1, count2)`.
- A FanOut that fans out to a single consumer is eliminated (replaced by a direct edge).
- A Barrier with `min_completion = 1.0` and no other consumers of its upstream FanOut is removed.

**Selection criteria coverage:**
- Measurable benefit: reduced scheduling overhead from unnecessary parallelism boundaries.
- Golden IR tests: 3+ snapshots (no consolidation needed, adjacent FanOuts, single-consumer FanOut).

### Step 5 — Wire optimization pipeline into compilation (0.5 day)

After `Strategy::lower()` produces `PrimitiveGraph`, run `OptimizationPipeline` before `to_execution_graph()`. In the compiler pipeline integration:

```
WorkflowIR
    → CompilerPass pipeline (as before)
    → Strategy::lower() produces PrimitiveGraph (as before)
    → OptimizationPipeline::run() [NEW]
    → PrimitiveGraph::to_execution_graph() (as before)
    → ExecutionGraph
```

The optimization pipeline is **opt-in** in v0.9.0-alpha (disabled by default) and **enabled by default** in v0.9.0-beta.

### Step 6 — Add optimization registry (1 day)

Create `OptimizationRegistry` (parallel to `StrategyRegistry`) that maps pass names to `Box<dyn OptimizationPass>`. Enable programmatic pass selection via config. Example:

```rust
// Config
optimization_passes = ["dead_node_elimination", "fanout_consolidation"]

// Registry
let registry = OptimizationRegistry::new();
registry.register(DeadNodeEliminationPass);
registry.register(FanOutConsolidationPass);
let pipeline = registry.build_pipeline(&config.optimization_passes)?;
```

---

## Related Documents

- ADR-003: Compiler — establishes the compiler pass pipeline and transactional rollback
- ADR-017: Execution Runtime ABI — establishes `ExecutionResult` as the runtime contract
- ADR-018: Strategy SDK — establishes `PrimitiveGraph`, `Strategy::lower()`
- ADR-019: PrimitiveGraph/ExecutionGraph Alignment — establishes `to_execution_graph()` and single canonical conversion
- `docs/roadmap-v0.9.md` — Phase 2 "Optimize" with dead node elimination and FanOut consolidation
- `src/compiler/optimization/mod.rs` — existing `OptimizationPass` trait and `OptimizationPipeline`
- `src/compiler/passes/mod.rs` — existing `CompilerPass` trait for WorkflowIR passes
- `src/compiler/ir/primitive_ir.rs` — `PrimitiveGraph` definition

---

## Unresolved Questions

1. **Should the optimization pipeline be configurable per-request, or is a global pipeline sufficient?** Per-request configurability enables A/B testing but adds complexity. The current design (global pipeline with opt-in flag) is simpler for v0.9.
2. **Should `PrimitiveGraph` carry an optimization manifest?** A `Vec<String>` of pass names that were applied would help with provenance and debugging. The trade-off is increased serialization size for golden IR snapshots.
3. **Should analysis passes be separate `OptimizationPass` implementations, or should analysis be embedded in the optimization pass that consumes it?** Embedding analysis simplifies the pipeline (fewer passes) but makes the analysis non-reusable. The answer depends on whether multiple optimization passes need the same analysis — for now, embed analysis in the consuming pass.
4. **Who owns the OptimizationRegistry?** Should it live under `compiler::optimization::registry`, or should it be a standalone module? The compiler module is the natural home.
