# v0.9 Roadmap

**Guiding Principle:** Every execution is reproducible, explainable, and compiler-verifiable.

Based on structured debate and consensus analysis of v0.8.0 status (see `debate_architecture.md`).

## Architectural Objectives

These are the primary architectural refinements that v0.9 must address, ranked by impact. They are not tasks — they are constraints that all Phase 0-3 work must respect.

| Rank | Objective | Rationale |
|------|-----------|-----------|
| **O-1** | **Eliminate PrimitiveGraph/ExecutionGraph drift by making `ExecutionGraph` a direct lowering target of `PrimitiveGraph`** | The compiler is now the architectural center. `primitive_to_subgraph()` is a transitional converter — the Phase 2 bridge. The end state is: `lower()` produces `PrimitiveGraph` → compiler optimizes `PrimitiveGraph` → `ExecutionGraph` is mechanically derived from `PrimitiveGraph` with no independent mutability. Any divergence between the two produces silent correctness failures and must be structurally impossible, not just documented away. |
| **O-2** | **Complete ADR-018 migration: retire `apply()` from `Strategy` trait** | All consumers must use `lower()`. The dual-path executor (`resolve_strategy()` preferring `lower()` over `apply()`) was a transitional design; Phase 3 makes it architectural. |
| **O-3** | **Make compiler passes verifiable** | Compiler passes transform `WorkflowIR` → `WorkflowIR` with transaction rollback. Every pass must document its pre/post invariants and be independently testable via golden IR snapshots. |
| **O-4** | **Provenance-first artifact model** | Every `ExecutionResult` must carry the compiler provenance (graph hash, pass manifest, PrimitiveGraph version) that produced it. This enables audit, replay, and debugging without shared state. |

## Completed in v0.8.x Working Tree

| Item | Description |
|------|-------------|
| ADR-018 Phase 1 | Strategy SDK: `descriptor()`/`lower()` on all 6 strategies, `PrimitiveGraph`, `StrategyIR`, `StrategyRegistry`, golden IR snapshots |
| ADR-017 Runtime ABI | `ExecutionResult`/`ExecutionInstance` terminal node fields, single `run_inner()` scheduler loop |
| Compiler refactoring | `passes.rs` decomposed into `ir/`, `context/`, `diagnostics/`, `registry/`, `passes/`, `optimization/` |
| Metrics safety | Refactored from macros to safe registration helpers |
| **ADR-018 Phase 2** | Executor `resolve_strategy()` prefers `lower()` over `apply()`, `primitive_to_subgraph()` converter |
| **FusionStrategy** | Implemented parallel multi-strategy fusion with `FanOut` → `Barrier` → `Reducer` lowering |
| **Aggregate removed** | Dead `ExecutionNodeKind::Aggregate` variant removed |
| **Test gap closed** | 12 inline `apply()` tests restored across chain, react, reflection, debate strategies |

## Phase 0 — Foundation

Before any stabilization work begins, the following architectural decisions must be made. These are P0 because every downstream phase depends on them.

| Priority | Item | Effort |
|----------|------|--------|
| P0 | **Define O-1 implementation approach**: Select one of:
  - (A) **Mechanical derivation**: `ExecutionGraph` becomes a pure lowering target — `lower()` produces `PrimitiveGraph`, compiler optimizes it, then `ExecutionGraph` is generated in a single pass with no separate mutability
  - (B) **Strict invariant enforcement**: Both graphs coexist but a compiler pass validates equivalence before execution
  - (C) **Merge**: Eliminate `ExecutionGraph` entirely; scheduler reads `PrimitiveGraph` directly | 1 day |
| P0 | **Assign scheduler migration owner**: Resolve who owns the `PrimitiveGraph` → scheduler path and timeline. The open question from Phase 2 — "Who owns the scheduler migration?" — must be closed before Phase 1 begins. | 0.5 day |
| P0 | **Define optimization pass selection criteria**: Explicitly document what qualifies an optimization pass for inclusion (performance impact, complexity ceiling, test coverage requirements) before implementation begins in Phase 2. | 0.5 day |
| P0 | **Clarify ResourceGuard contract**: Document whether `ResourceGuard` RAII Drop is relied upon for release at `pipeline.rs:167-168`, or if explicit `commit()` is the sole release path. | 0.5 day |
| P1 | **Define provenance schema**: Specify which fields every `ExecutionResult` carries (graph hash, PrimitiveGraph version, pass manifest, strategy descriptor). Align with O-4 before Phase 1 implementation begins. | 0.5 day |

## Phase 1 — Stabilize (v0.9.0-alpha)

| Priority | Item | Effort |
|----------|------|--------|
| P0 | **Architectural Objective O-1**: Implement the approach selected in Phase 0. Make `ExecutionGraph` a direct lowering target of `PrimitiveGraph` — `primitive_to_subgraph()` becomes the single canonical conversion path. | 3-5 days |
| P0 | **ADR-018 Phase 3**: Retire `apply()` from `Strategy` trait. All consumers use `lower()`. | 3-5 days |
| P0 | **Document migration**: Add deprecation notice to `apply()`, update all plugin examples | 1 day |
| P0 | **Fix unused imports warnings**: Clean up `#[allow(unused_imports)]` in `compiler/ir/mod.rs` | 0.5 day |
| P1 | **FusionStrategy integration tests**: Add golden tests for FusionStrategy `apply()` and `lower()` | 0.5 day |
| P1 | **PrimitiveGraph scheduler tests**: Add test for `resolve_strategy()` preferring `lower()` path | 1 day |
| P1 | **Operator docs**: Create deployment guide, Dockerfile, scaling guidance | 2 days |

## Phase 2 — Optimize (v0.9.0-beta)

| Priority | Item | Effort |
|----------|------|--------|
| P1 | **Optimization passes**: Implement at least 2 concrete `OptimizationPass` impls (e.g., dead node elimination, FanOut consolidation) | 3-5 days |
| P2 | **Artifact trait integration**: Wire `Artifact` trait into execution model — store typed artifacts per node | 2 days |
| P2 | **FusionStrategy refinement**: Add dynamic sub-strategy selection based on model availability | 1-2 days |

## Phase 3 — Mature (v0.9.0)

| Priority | Item | Effort |
|----------|------|--------|
| P2 | **ADR-007 through ADR-012**: Document missing architectural decisions | 2 days |
| P2 | **User-facing logging and monitoring**: Expose execution telemetry via the `/metrics` endpoint — per-strategy latency histograms, error rates, graph hash distribution. Add structured request/response logging for operator observability. | 2-3 days |
| P2 | **WASM plugin wiring**: Connect `wasmtime` runtime to `PluginManager` — load and register WASM strategies | 3-5 days |
| P2 | **Benchmark suite expansion**: Add strategy-specific benchmarks (lower() vs apply() throughput comparison) | 2 days |
| P3 | **Remove dead code paths**: Clean up remaining dead abstractions after migration | 1 day |

## Open Questions from Debate

1. **Production deployments**: How many real users exist? If zero, the churn is acceptable pre-1.0. This informs whether Phase 3 user-facing features should be prioritized over internal maturity work.
