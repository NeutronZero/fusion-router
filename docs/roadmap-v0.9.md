# v0.9 Roadmap

Based on structured debate analysis of v0.8.0 status (see `debate_architecture.md`).

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

## Phase 1 — Stabilize (v0.9.0-alpha)

| Priority | Item | Effort |
|----------|------|--------|
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
| P1 | **PrimitiveGraph → ExecutionGraph bridge**: Make `primitive_to_subgraph()` handle all edge cases (nested FanOut, multi-level Barrier chaining) | 2-3 days |
| P2 | **Artifact trait integration**: Wire `Artifact` trait into execution model — store typed artifacts per node | 2 days |
| P2 | **FusionStrategy refinement**: Add dynamic sub-strategy selection based on model availability | 1-2 days |

## Phase 3 — Mature (v0.9.0)

| Priority | Item | Effort |
|----------|------|--------|
| P2 | **ADR-007 through ADR-012**: Document missing architectural decisions | 2 days |
| P2 | **WASM plugin wiring**: Connect `wasmtime` runtime to `PluginManager` — load and register WASM strategies | 3-5 days |
| P2 | **Benchmark suite expansion**: Add strategy-specific benchmarks (lower() vs apply() throughput comparison) | 2 days |
| P3 | **Remove dead code paths**: Clean up remaining dead abstractions after migration | 1 day |

## Open Questions from Debate

1. **Phase 2-3 timeline**: Who owns the scheduler migration to `PrimitiveGraph`? Without a concrete plan, the SDL infrastructure remains ornamental.
2. **Production deployments**: How many real users exist? If zero, the churn is acceptable pre-1.0.
3. **ResourceGuard contract**: At `pipeline.rs:167-168`, the guard is created and discarded — is RAII Drop relied upon for release?
4. **PrimitiveGraph/ExecutionGraph drift**: Optimizers could produce no observable effect if they transform `PrimitiveGraph` but the scheduler reads `ExecutionGraph`.
