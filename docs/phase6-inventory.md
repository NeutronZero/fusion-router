# Phase 6 — Inventory & Call-Graph Freeze (6.0)

Status: as of commit after Phase 6 PR1/PR2 delegation. Source of truth: `master` (pushed directly; no PR flow).

## Goal

`src/` becomes a thin binary/host (HTTP, config, providers, CLI). Execution plane = crates only:

```
fusion-ir (planning, immutable)
    ↓ adapter (src/ir/adapter.rs — the one boundary)
fusion-types
    ↓
fusion-compiler / fusion-planner / fusion-scheduler / fusion-runtime
    ↓
src/ (binary + providers + API)
```

## 6.0.1 — Production entrypoints → call chain

| Entrypoint | Planner | Compiler | Scheduler | Executor | Provider |
|---|---|---|---|---|---|
| `POST /v1/chat/completions` (`src/server/handlers/chat.rs:25`) | `[src] IntentPlanner::plan` (`pipeline.rs:114` → `planner/intent_planner.rs:419`) | `[crates→src] build_compiler` (`state.rs:74`, pipeline.rs:127) | `[src] DefaultScheduler` (`pipeline.rs:238`, `scheduler/default.rs:41`) | `[src] DefaultExecutor` (`node_exec.rs:119`) | `[src] providers/mod.rs:256` |
| `POST /v1/messages` (`handlers/anthropic.rs:21`) | same `process_request` as chat | same | same | same | same |
| `POST /v1/executions` (`server/execution.rs:293`) | **none** (IR from body) | `[crates→src] build_compiler` (`main.rs:191-197`, `execution.rs:130`) | **none** — hand-rolled topo loop (`execution.rs:70-104`) | `[src] DefaultExecutor` (`execution.rs:188`) | same |
| Streaming chat/anthropic | — | — | — | — | direct `provider.chat_stream` (no graph) |
| `fusion-router review` CLI (`review.rs:181`) | none (hand-built node) | Consensus expansion only — now `[crates] fusion_compiler::strategy_expansion` (`review.rs:253`) | `[src] DefaultScheduler::new(1)` (`review.rs:272`) | `[src] DefaultExecutor` | `[src] ProviderRegistry` |
| `fusion` CLI / `eval_runner` bin | — | — | — | — | release governance / dev harness, no runtime pipeline |
| Triggers (`src/trigger/engine.rs`) | — | `CompilerPipeline` (src) | — | — | **not wired into production** |

## 6.0.2 — Swap points (src → crates)

| # | Swap | src location | crates target | Status |
|---|---|---|---|---|
| 1 | Compiler factory + pipeline | `src/compiler/mod.rs` `build_compiler` / `DefaultCompiler::compile` | `fusion_compiler::build_compiler` passes + `fusion_compiler::lower_to_graph` (aligned incl. `dead_node_elimination`) | ✅ DONE (PR2) |
| 2 | Resource bridge | `src/resource::ResourceManager` (graph-shaped) | `fusion_kernel::resource::ResourceManager` (scalar) via `src/resource/kernel_adapter.rs` | ✅ DONE (PR1) |
| 3 | Policy bridge | `src/policy::ir::PolicyIR` | `fusion_compiler::policy::PolicyIR` via `src/policy/bridge.rs` `From` | ✅ DONE (PR1) |
| 4 | Strategy expansion (compile-time) | `src/compiler/strategy_expansion` (7 kinds; dead in lib, kept for runtime `strategy_ir_from_node`) | `fusion_compiler::strategy_expansion` (Consensus) | ✅ DONE (PR2); 6.6 cleanup pending |
| 5 | Scheduler | `src/scheduler/default.rs` (`schedule`, `run_with_cancellation`) | `fusion_scheduler::DefaultScheduler` — semantic parity ported (cancellation, Conditional, Loop, loop-back caps, per-token cost) in commit `8f00021` | ⏳ PR3b: flip call sites — blocked on budget-envelope-in-crates + retry/fallback wrapper parity |
| 6 | Executor / runtime | `src/executor/node_exec.rs` `DefaultExecutor` | `fusion_runtime::RuntimeEngine` + `ProviderExecutor`; `ChatProvider` wrappers for real providers | ⏳ PR3/PR4 |
| 7 | Planner | `src/planner/intent_planner.rs` `IntentPlanner::plan` | `fusion_planner::PlannerService` | ⏳ PR5 |
| 8 | `/v1/executions` scheduler gap | hand-rolled topo loop (`execution.rs`) | insert crates scheduler hop | ⏳ PR3 |

## 6.0.3 — Key call sites (grep results, authoritative)

- `build_compiler`: `src/main.rs:192`, `src/server/pipeline.rs:400,447` (tests), `src/server/handlers/state.rs:74`, `src/server/execution.rs:387` (test), `src/bin/eval_runner.rs:1491`
- `DefaultCompiler`: `src/compiler/mod.rs:20`, `src/server/handlers/state.rs:5`, `tests/{golden/dag.rs:10, golden/compiler.rs:34, load_test.rs:496, security_invariants.rs:8, contract_wiring.rs:17}`, `benches/compilation.rs:9`
- Scheduler/executor: `src/server/handlers/state.rs:138-146`, `src/server/pipeline.rs:238-243`, `src/scheduler/default.rs`, `src/executor/node_exec.rs`, `src/review.rs:272-276`
- No `Ok(ir.clone())` stub passes exist in production crates (grep verified). `fusion-runtime` has a pre-existing `total_cost` unused-var warning (Phase 4.6 debt; crates audit item).

## 6.0.4 — Test split (`tests/`)

- **[src-monolith]**: `config_reload_tests` (⚠ pre-existing failure, env-var prepare semantics, fails at HEAD before Phase 6), `contract_wiring`, `ir_shim`, `load_test`, `package_tests`, `planner_lowering`, `release_*`, `reliability_tests`, `runtime_events_tests`, `runtime_tests`, `security`, `security_invariants`, `slo_tests`, `streaming_tests`
- **[crates-workspace]**: `beta_*` (dashboard/first_run/health/provider_setup/replay), `capability_sdk_integration`, `compatibility_v1`, `placement_validation`, `runtime_resilience`, `scheduler_validation`
- **[hybrid]**: `beta_chat`, `beta_inspector`, `beta_integration`, `domain_invariants`, `e2e_golden`, `equivalence_suite`, `host_tests`, `performance_slo`, `replay_validation`

## Verification (6.7 status)

- `cargo test --workspace`: green except pre-existing `config_reload_tests::test_provider_registry_rejects_bad_prepare` (fails at HEAD too)
- `cargo build --examples`: green
- Parity coverage: `phase6_consensus_expands_through_crates_lower`, `phase6_dead_node_elimination_is_live` (src/compiler/mod.rs), bridge unit tests (src/policy/bridge.rs, src/resource/kernel_adapter.rs), Law 1/2/4 tests unchanged and passing
- `fusion-scheduler` parity (commit `8f00021`): 17 tests green — conditional edge activation, loop continue/exit, loop-back iteration caps, pre-run + mid-run cancellation, per-token cost

## PR3b gate (scheduler delegation) — not yet flipped

The crates scheduler now matches the monolith loop semantics, but the production flip is still blocked on two parity items, per the risk-mitigation rule (parity tests before delegation):

1. **Budget envelope**: `src/resource::BudgetEnvelope` (iteration limit + `record_and_check`) is enforced inside the monolith `run_inner` loop. The crates `run` has no such hook; envelope type must be lifted to `fusion_types` + consumed by `fusion_scheduler` (or enforced per-node in the executor boundary).
2. **Retry/fallback**: monolith scheduler retries nodes with exponential backoff and attempts `node.fallback` at scheduler level (`default.rs:313-390`). The crates path relies on `fusion_runtime::ProviderExecutor` for retry/fallback — the src `DefaultExecutor` does not. A flip needs a retry/fallback wrapper executor at the src boundary (or porting retry/fallback into `DefaultExecutor`).

## Known debt (deliberate, this phase)

- `src/compiler/strategy_expansion` full 7-kind lowering kept alive by executor runtime fallback (`strategy_resolver.rs:117` uses `strategy_ir_from_node`); dead entry points marked `#[allow(dead_code)]`, removed in 6.6
- `src/compiler/passes/*` (legacy passes) kept for the unwired trigger `CompilerPipeline`; deleted with it in 6.6
- `CompilerPipeline` / `TriggerExecutionEngine` not wired into production (pre-existing)
