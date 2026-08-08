# FusionRouter Compiler

## Overview

The compiler is a **pipeline of pure, deterministic passes** that lowers `WorkflowIR` into an `ExecutionGraph`. It has no side effects, makes no LLM calls, and is fully deterministic (same input → same output).

**Location:** `src/compiler/`
**Key types:** `Compiler` trait, `DefaultCompiler`, `CompilerPass` trait
**Design doc:** `docs/specifications/compiler-passes.md`

## IR Representations

| IR | Stage | Description |
|----|-------|-------------|
| `WorkflowIR` | Input | High-level abstract plan with semantic node types (Generate, Review, etc.) |
| `StrategyIR` | On-node | Strategy declaration carried as a field on every IR/execution node (Single, Consensus, Reflection, Chain, ReAct, Debate, Fusion, Custom) |
| `PrimitiveGraph` | Compiler-internal | IR produced by `Strategy::lower`; used for per-node strategy expansion. Expanded into an `ExecutionSubgraph` by `strategy_expansion` during compilation |
| `ExecutionGraph` | Output | Fully resolved executable form: 10 concrete node kinds, resolved models, retry policies, deterministic UUIDs |

## Standard Passes (in order)

Constructed exclusively via `build_compiler()` (`src/compiler/mod.rs`) — the sole production construction path (ADR-034). The optional policy pass is appended when a `PolicyIR` is supplied.

1. **ConstraintValidation** — Rejects empty IR (must have at least one node)
2. **ControlFlowValidation** — Validates edge targets reference known nodes; Conditional/Loop/Split/Join/Barrier shape rules (conditional edges, `max_iterations` on loops, ≥2 outgoing on split, ≥2 incoming on join, in/out on barrier); three-color DFS acyclicity check (loop back-edges exempt)
3. **ModelResolution** — Fills `model: None` from the `ModelCatalog` (tools/high-coding → code model, high-reasoning → architecture model, else fast)
4. **BudgetOptimisation** — Checks `ResourceManager::can_afford` against estimated cost/tokens; rejects when the budget is exceeded
5. **PolicyCompilerPass** *(optional)* — Applies compiled `PolicyIR` to the IR; any matched Deny rule is a compile error (Law 2)

After the pass loop, `lower_to_graph` (`src/compiler/mod.rs`) performs the final **direct structural lowering** `WorkflowIR` → `ExecutionGraph`: 1:1 node-kind mapping, strategy copied through as a node field, `primitive_graph_hash` = 0. As a final compile step, `strategy_expansion` (`src/compiler/strategy_expansion.rs`) materializes each strategy node's subgraph: `expanded_subgraph` looks the strategy up in the default `StrategyRegistry`, calls `Strategy::lower` → `PrimitiveGraph::to_execution_graph`, and attaches the deterministic result to `node.subgraph`. The executor consumes `node.subgraph` (propagating the parent's assembled `messages` into every LLM sub-node via `DefaultExecutor::propagate_parent_messages` before dispatch — subgraphs are built without request context); runtime lowering survives only as a legacy fallback for graphs compiled before expansion was added.

## Optimization Framework

**Location:** `src/compiler/optimization/`
**ADR:** ADR-020 (Accepted)

6-pass taxonomy with 7 selection criteria:

1. **Validation passes** — Structural correctness before transformation
2. **Lowering passes** — High-level IR → primitive form
3. **Analysis passes** — Collect metrics/information without mutation
4. **Optimization passes** — Transform for performance/cost
5. **Instrumentation passes** — Add observability probes
6. **Verification passes** — Validate post-optimization invariants

Selection criteria: Training signal, Pattern detection, Cost model, Heuristic, User annotation, Policy requirement, Safety requirement.

Legality rules govern pass ordering. Rollback safety ensures recovery from failed optimizations.

> **Status note:** the optimization framework (`DeadNodeEliminationPass`, `FanOutConsolidationPass`) operates on `PrimitiveGraph` and is **not wired into the production pipeline** — it is exercised only by its own unit tests. The live pipeline is the fixed `build_compiler` pass list above.

## Key Invariants

- Compiler is 100% pure: no I/O, no LLM calls, no side effects
- Deterministic: same `WorkflowIR` input always produces same `ExecutionGraph`
- `ExecutionGraph` is frozen after compilation — never mutated by scheduler or executor
- Compiler owns graph lifecycle
- The pass pipeline is fixed in `build_compiler`; `PassManager` (`src/compiler/passes/mod.rs`) is a helper for composing pass lists but is not used by the production path
- `PrimitiveGraph`/`to_execution_graph()` is used for strategy expansion, performed by the compiler at compile time via `strategy_expansion` (ADR-019 fulfilled; executor keeps runtime lowering only as a legacy fallback for pre-expansion graphs)
- A workflow violating a matched Deny policy rule cannot produce an `ExecutionGraph` (ADR-034, v0.13.1)
- Every execution endpoint compiles through the shared `build_compiler()` pass pipeline; no production path constructs `DefaultCompiler` with an empty pass list (ADR-034, v0.13.1)
- Phase 1 law tests: `law1_build_compiler_*` / `law1_build_compiler_produces_mandatory_passes`, `law2_deny_blocks_compilation`, `law4_compile_failure_yields_no_graph` (unit, `src/compiler/mod.rs`), `law5_execution_plane_uses_full_passes` + rejection cases (end-to-end, `tests/security_invariants.rs`) — all green as of v0.13.1 Phase 1 (2026-08-03)

## Source Files

| File | Purpose |
|------|---------|
| `src/compiler/mod.rs` | `Compiler` trait, `DefaultCompiler`, `build_compiler` (pass pipeline), `lower_to_graph` |
| `src/compiler/pipeline.rs` | `CompilerPipeline` helper (not used by the production path) |
| `src/compiler/context/mod.rs` | Compiler context |
| `src/compiler/diagnostics/mod.rs` | Compiler diagnostics |
| `src/compiler/ir/mod.rs` | IR module root |
| `src/compiler/ir/primitive_ir.rs` | `PrimitiveGraph` definition (deterministic `to_execution_graph`) |
| `src/compiler/strategy_expansion.rs` | Default `StrategyRegistry` (7 built-ins), `expanded_subgraph` — pre-builds each strategy node's `ExecutionSubgraph` at compile time |
| `src/compiler/ir/strategy_ir.rs` | `StrategyIR` definition |
| `src/compiler/passes/mod.rs` | Pass trait + `PassManager` helper |
| `src/compiler/passes/legacy_passes.rs` | The 4 mandatory pass implementations |
| `src/compiler/passes/policy.rs` | Optional policy compilation pass |
| `src/compiler/optimization/mod.rs` | Optimization framework (not wired into the production pipeline) |
| `src/compiler/registry/mod.rs` | `StrategyRegistry` (used by `strategy_expansion` for the default built-ins) |

## Related ADRs

- ADR-003: Compiler architecture, pure pass pipeline
- ADR-018: Strategy SDK, two-layer IR (StrategyIR, PrimitiveIR)
- ADR-019: PrimitiveGraph as canonical, ExecutionGraph derivation (compile-time via `strategy_expansion`)
- ADR-020: Compiler optimization framework taxonomy (not yet wired in)
- ADR-027: Compiler phase invariants matrix ("May Do" / "Must Not Do")
- ADR-034: Single compiler pipeline — `build_compiler()` sole construction path
