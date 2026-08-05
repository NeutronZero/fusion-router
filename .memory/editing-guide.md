# Editing Guide

> Operational knowledge: where to edit for every component and common task.

## How to Use

Each entry specifies:

- **Read** — Architecture docs to load first
- **Files** — Source files to edit
- **May affect** — Components that could be impacted by changes
- **Never modify** — Components that must stay untouched
- **Tests** — Test locations
- **Check** — Invariants to verify after changes

---

## Core Pipeline Components

### Planner

| | |
|---|---|
| **Read** | `planner.md`, `adrs.md` (ADR-002, 013, 014, 015) |
| **Files** | `src/planner/` (all), `src/types/execution.rs` (WorkflowIR) |
| **May affect** | Compiler (WorkflowIR shape), CapabilityResolver |
| **Never modify** | Runtime, Scheduler, Executor |
| **Tests** | `tests/` (planner integration), `src/planner/*.rs` (unit) |
| **Check** | Planner produces WorkflowIR not ExecutionGraph. No provider references in output. |

### Compiler

| | |
|---|---|
| **Read** | `compiler.md`, `architecture.md`, `adrs.md` (ADR-003, 018, 019, 020, 027) |
| **Files** | `src/compiler/` (all), `src/types/execution.rs` (ExecutionGraph) |
| **May affect** | Scheduler (ExecutionGraph structure), policies (pass ordering) |
| **Never modify** | Planner (compiler receives WorkflowIR, never generates it). Provider. Runtime. |
| **Tests** | `tests/` (compiler integration), `src/compiler/**/*.rs` (unit) |
| **Check** | Compiler must stay pure (no LLM calls, no I/O). Deterministic: same input → same output. ExecutionGraph frozen after compilation. |

### Scheduler

| | |
|---|---|
| **Read** | `scheduler.md`, `runtime.md`, `adrs.md` (ADR-004, 017) |
| **Files** | `src/scheduler/` (all) |
| **May affect** | Executor (dispatch contract), ConnectorResolver |
| **Never modify** | Compiler (scheduler reads frozen ExecutionGraph). Planner. |
| **Tests** | `tests/` (scheduler integration), `src/scheduler/*.rs` (unit) |
| **Check** | Scheduler never mutates ExecutionGraph. Topology-driven. Output selection owned by scheduler. |

### Executor

| | |
|---|---|
| **Read** | `execution.md`, `capability-system.md` |
| **Files** | `src/executor/` (all) |
| **May affect** | Provider, Strategy, Tool, Connector dispatch. Session state. |
| **Never modify** | Compiler, Planner, Scheduler scheduling logic. |
| **Tests** | `tests/` (executor), `src/executor/*.rs` (unit) |
| **Check** | Dispatches via CapabilityExecutor. Resource cleanup via RAII guards. |

---

## Subsystems

### Capability System

| | |
|---|---|
| **Read** | `capability-system.md`, `adrs.md` (ADR-021, 022, 023, 028) |
| **Files** | `src/capability/`, `src/planner/resolver/capability/`, `crates/fusion-capability-sdk/`, `crates/fusion-capability-macros/`, `crates/fusion-plugin-api/` |
| **May affect** | Compiler (pass pipeline), Execution (CapabilityExecutor), Plugin system |
| **Never modify** | Planner intent types. Scheduler output selection. |
| **Tests** | `tests/`, crate-level tests in SDK crates |
| **Check** | Registry freezes after startup. Late-bound resolution. ABI version compatibility. |

### Provider System

| | |
|---|---|
| **Read** | `providers.md`, `adrs.md` (ADR-001, 005) |
| **Files** | `src/providers/`, `src/transport/` |
| **May affect** | Executor (dispatch routing), Strategies (model selection) |
| **Never modify** | Compiler, Planner |
| **Tests** | `tests/` (provider integration), `src/providers/*.rs` (unit) |
| **Check** | All LLM calls go through Provider trait. Circuit breaker state transitions correct. |

### Policy System

| | |
|---|---|
| **Read** | `policies.md`, `adrs.md` (ADR-024) |
| **Files** | `src/policy/`, `src/release/`, `src/compiler/passes/policy.rs` |
| **May affect** | Compiler (policy pass), Release governance (gates, attestation) |
| **Never modify** | Core pipeline stage logic. |
| **Check** | Policy expressions compile to valid IR. Precedence engine resolves conflicts. |

### Event System & Telemetry

| | |
|---|---|
| **Read** | `telemetry.md` |
| **Files** | `src/events/`, `src/telemetry/` |
| **May affect** | Session (checkpoint events), Release (attestation evidence) |
| **Never modify** | Core pipeline. Events must be non-blocking. |
| **Check** | Events immutable after emission. Telemetry never blocks. Evidence written post-execution. |

### Plugin System

| | |
|---|---|
| **Read** | `plugin-system.md`, `adrs.md` (ADR-010, 022) |
| **Files** | `src/plugin/`, `src/wasm/` (feature-gated) |
| **May affect** | Capability Registry (plugin registration), Compiler (pass plugins) |
| **Never modify** | Core pipeline ordering. |
| **Check** | Plugin discovery at startup only. No hot-reload. ABI version check. |

### Session & Lifecycle

| | |
|---|---|
| **Read** | `execution.md`, `runtime.md`, `adrs.md` (ADR-026, 029, 030) |
| **Files** | `src/session/`, `src/lifecycle/` |
| **May affect** | Executor (session context), Checkpoint (event triggers) |
| **Never modify** | Compiler, Planner, Scheduler scheduling. |
| **Check** | Session identity/snapshot separation. Replay mode correctness. |

---

## Common Tasks

### "Add a new compiler pass"

```
Read:  compiler.md, adrs.md (ADR-003, 020, 027, 034)
Edit:  src/compiler/passes/ (new pass file)
       src/compiler/passes/mod.rs (register)
       src/compiler/mod.rs (add to build_compiler pass order)
Check: Pass must be pure (no I/O, no LLM). Deterministic.
       Must declare "May Do" / "Must Not Do" per ADR-027.
       If mandatory, it must be added to build_compiler — no production
       path constructs DefaultCompiler outside it (ADR-034).
Tests: tests/ (compiler integration)
```

### "Add a new strategy type"

```
Read:  architecture.md (strategy table), adrs.md (ADR-018)
Edit:  src/strategies/ (new strategy file)
       src/strategies/mod.rs (register)
       src/compiler/ir/strategy_ir.rs (StrategyIR variant)
Check: Strategy implements lowering (StrategyIR → PrimitiveGraph).
       Strategy does not execute — lowering happens in the compiler's
       strategy_expansion (default_strategy_registry) at compile time;
       the executor consumes the prebuilt node.subgraph verbatim.
Tests: tests/ (strategy), src/strategies/{name}.rs (unit)
```

### "Add a new provider backend"

```
Read:  providers.md, adrs.md (ADR-005)
Edit:  src/providers/ (new model + provider files)
       src/providers/mod.rs (register)
       src/transport/ (if new transport needed)
Check: All LLM interactions through Provider trait.
       Transport abstracted from Model logic.
Tests: tests/ (provider integration)
```

### "Add a new connector"

```
Read:  runtime.md (connector types)
Edit:  src/connectors/ (new connector file)
       src/connectors/mod.rs (register)
Check: Connector may affect scheduler ConnectorResolver.
       Late-bound at execution time per ADR-025.
Tests: tests/ (connector)
```

### "Modify the planner"

```
Read:  planner.md, adrs.md (ADR-002, 013, 014, 015)
Edit:  src/planner/ (relevant planner file)
       src/types/execution.rs (if changing WorkflowIR)
Check: Planner produces WorkflowIR, never ExecutionGraph.
       Planner may call LLMs (DynamicPlanner).
       Planner must not reference providers or connectors.
Tests: tests/ (planner integration)
```

### "Modify release governance"

```
Read:  policies.md (release gates section)
Edit:  src/release/ (relevant gate, policy, attestation)
       src/devex/commands/gates.rs (CLI commands)
Check: 8 release gates. 4-phase attestation verification.
       Environment levels (Dev/Staging/Production).
Tests: tests/ (release), src/release/*.rs (unit)
```

---

## Safety: Things You Should Almost Never Do

| Action | Reason |
|--------|--------|
| Planner writes ExecutionGraph | Planner produces WorkflowIR. Compiler produces ExecutionGraph. (ADR-002) |
| Compiler calls an LLM | Compiler must be pure and deterministic. No I/O. (ADR-003) |
| Runtime mutates ExecutionGraph | Graph is frozen after compilation. Scheduler reads only. (ADR-017) |
| Capability resolution at planning time | Resolution is late-bound at compilation. (ADR-023) |
| Plugin hot-reload | Plugin discovery at startup only. No runtime registration. (ADR-010) |
| Bypass Provider trait for LLM calls | All LLM interactions go through Provider. (Invariant #1) |
| Circular capability dependencies | CapabilityGraph must remain a DAG. |
| Event emission blocks execution | Telemetry never blocks request processing. |
| Config changes at runtime | Configuration validated at startup, immutable at runtime. |
