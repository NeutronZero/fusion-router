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
| `StrategyIR` | Mid | Strategy-aware IR: 6 strategy kinds (Single, Consensus, Reflection, Chain, ReAct, Debate, Fusion) |
| `PrimitiveGraph` | Mid | Canonical lowered IR — atomic primitive operations, strategy-expanded subgraphs |
| `ExecutionGraph` | Output | Fully resolved executable form: 12 concrete node kinds, resolved models, retry policies, deterministic UUIDs |

## Standard Passes (in order)

1. **ConstraintValidation** — Validates WorkflowIR: no cycles, valid entry/exit, correct edge types
2. **CapabilityResolution** — Resolves abstract capability references to `CapabilityInstance` via `CapabilityResolver`
3. **BudgetOptimisation** — Applies resource budgets (token limits, timeouts) from policy
4. **NodeFusion** — Merges compatible sequential nodes into single execution units
5. **RetryFallbackInsertion** — Wraps nodes with retry policies, fallback paths from `NodeMetadata` annotations
6. **SchedulingHints** — Annotates nodes with scheduling metadata (affinity, priority)
7. **GraphVerification** — Validates final `ExecutionGraph`: connectivity, type safety, invariant checks

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

## Key Invariants

- Compiler is 100% pure: no I/O, no LLM calls, no side effects
- Deterministic: same `WorkflowIR` input always produces same `ExecutionGraph`
- `ExecutionGraph` is frozen after compilation — never mutated by scheduler or executor
- Compiler owns graph lifecycle
- Pass pipeline is extensible via `PassRegistry` (ADR-010)
- PrimitiveGraph → ExecutionGraph derivation via `to_execution_graph()` (ADR-019)
- A workflow violating a matched Deny policy rule cannot produce an `ExecutionGraph` (ADR-034, v0.13.1)
- Every execution endpoint compiles through the shared `build_compiler()` pass pipeline; no production path constructs `DefaultCompiler` with an empty pass list (ADR-034, v0.13.1)

## Source Files

| File | Purpose |
|------|---------|
| `src/compiler/mod.rs` | `Compiler` trait, `DefaultCompiler` |
| `src/compiler/pipeline.rs` | Pass pipeline orchestration |
| `src/compiler/context/mod.rs` | Compiler context |
| `src/compiler/diagnostics/mod.rs` | Compiler diagnostics |
| `src/compiler/ir/mod.rs` | IR module root |
| `src/compiler/ir/primitive_ir.rs` | `PrimitiveGraph` definition |
| `src/compiler/ir/strategy_ir.rs` | `StrategyIR` definition |
| `src/compiler/passes/mod.rs` | Pass trait + standard passes |
| `src/compiler/passes/legacy_passes.rs` | Legacy pass implementations |
| `src/compiler/passes/policy.rs` | Policy compilation pass |
| `src/compiler/optimization/mod.rs` | Optimization framework |
| `src/compiler/registry/mod.rs` | Pass registry |

## Related ADRs

- ADR-003: Compiler architecture, pure pass pipeline
- ADR-018: Strategy SDK, two-layer IR (StrategyIR, PrimitiveIR)
- ADR-019: PrimitiveGraph as canonical, ExecutionGraph derivation
- ADR-020: Compiler optimization framework taxonomy
- ADR-027: Compiler phase invariants matrix ("May Do" / "Must Not Do")
