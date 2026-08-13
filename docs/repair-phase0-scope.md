# Phase 0: Scope Freeze — Repair Decisions

## Two IR Contracts (Permanent)

| Contract | Crate | Purpose | Properties |
|----------|-------|---------|------------|
| Planning IR | `fusion-ir` | Provider-free workflow definition | Immutable, String IDs, BTreeMap config, sealed |
| Execution IR | `fusion-types` | Compiler/scheduler/runtime graph | Mutable pub fields, Uuid IDs, HashMap config |

**Adapter stays permanently** at `src/ir/adapter.rs` as the only planning → execution boundary.

## Naming Convention

At call sites where both are in scope:
```rust
use fusion_ir::WorkflowIR as PlanningIR;
use fusion_types::WorkflowIR as ExecIR;
```

## Pass Triage

| Pass | Action | Source |
|------|--------|--------|
| ConstraintValidation | **Port** | `src/compiler/passes/legacy_passes.rs:12-27` |
| ControlFlowValidation | **Port** (3-color DFS) | `src/compiler/passes/legacy_passes.rs:109-260` |
| ModelResolution | **Port** | `src/compiler/passes/legacy_passes.rs:29-71` |
| BudgetOptimisation | **Port** | `src/compiler/passes/legacy_passes.rs:76-107` |
| PolicyCompiler | **Port** (Phase 3) | `src/compiler/passes/policy.rs` |
| DeadNodeElimination | **Port** (Phase 3) | `src/compiler/optimization/` |
| ConstantFolding | **Delete** | No design, no src/ impl |
| NodeFusion | **Delete** | No design, no src/ impl |
| RetryInjection | **Delete** | Scheduler concern, not compiler |
| FallbackInjection | **Delete** | Scheduler concern |
| SchedulingHints | **Delete** | No design, no src/ impl |
| ConstraintSolver | **Delete** | No design, no src/ impl |
| CapabilityResolution | **Port later** (Phase 5) | Partial impl exists |

## Provider Trait Boundary for Runtime

`fusion-runtime` depends on:
- `fusion-types` (graph types)
- A small `ChatProvider` trait (extract from `src/providers/mod.rs`)
- Mock provider for tests

Real connectors/providers stay in `src/` or behind trait objects injected by the binary.

## Golden Workflow

**Input**: `"Build a web app"` + `ExecutionIntent::Balanced`

**Pipeline**:
1. Planner → 3-node IR (gen → gen → judge)
2. Compiler → 4 mandatory passes → `lower_to_graph`
3. Scheduler → DAG execution via WorkQueue
4. Runtime → mock provider (fixed text + usage)

**Assertions**:
- `success == true`
- 3 nodes in execution graph
- CompilerReport lists 4 real passes (not stubs)
- No panics, no network calls

## Exit Criteria

| Criterion | How to verify |
|-----------|--------------|
| Golden E2E passes | `tests/e2e_golden.rs` green |
| ≥1 real compiler score | `capability_score` or `health_score` ≠ `None` |
| 4+ real compiler passes | ConstraintValidation, ControlFlowValidation, ModelResolution, BudgetOptimisation |
| No stub tests remaining | grep for `Completed` / hardcoded schedule strings in test assertions |
| Real scheduler | WorkQueue tests pass with fixed ExecutionGraph |
| Real runtime | Mock provider E2E test passes |
| `cargo test --workspace` green | All tests pass |

## Phase 2 Split (Sequential PRs)

1. `fusion-types` consumers compile (after Phase 1)
2. Compiler passes + `lower_to_graph` + unit tests
3. WorkQueue + scheduler unit tests
4. Runtime with mock provider only
5. Planner templates
6. Golden E2E last
