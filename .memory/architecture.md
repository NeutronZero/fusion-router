# FusionRouter Architecture

## System Architecture

FusionRouter follows a **compiler pipeline** architecture with discrete stages connected by well-defined IR boundaries.

### Request Lifecycle

```
Client Request
    │
    ▼
Server (Axum HTTP)
    │  Middleware: Auth → CORS → Rate Limit → Request ID
    ▼
ContextAssembler
    │  Builds ContextSnapshot(system prompt, history, tools)
    ▼
RequirementsExtractor
    │  Classifies intent, extracts complexity
    ▼
Planner
    │  WorkflowPlanner → SimplePlanner (fallback) → DynamicPlanner (optional)
    ▼
    WorkflowIR
    ▼
Compiler
    │  Pass pipeline (7 standard passes + optimizations)
    │  PrimitiveGraph → ExecutionGraph derivation
    ▼
    ExecutionGraph
    ▼
Scheduler
    │  WorkQueue DAG scheduler
    ▼
Executor
    │  CapabilityExecutor → Providers/Strategies/Tools/Connectors
    ▼
    ExecutionResult
```

### Data Models

| Type | Stage | Description |
|------|-------|-------------|
| `ContextSnapshot` | After Context Assembly | System prompt, conversation history, tool definitions |
| `Requirements` | After Requirements Extraction | Intent classification, complexity score |
| `WorkflowIR` | After Planning | Abstract plan: nodes (Generate, Review, Judge, Transform, Gate, Conditional, Loop, Split, Join, Barrier), edges with conditions |
| `PrimitiveGraph` | After Lowering | Canonical lowered IR, strategy-expanded subgraphs |
| `ExecutionGraph` | After Compilation | Executable form: 12 node kinds, resolved models, retry policies, deterministic UUIDs |
| `ExecutionInstance` | During Execution | Bound node + runtime params |
| `ExecutionResult` | After Execution | Standardized output + metrics |

## Compiler Pipeline (7 Standard Passes)

1. **Constraint Validation** — Validates WorkflowIR invariants: cycles, entry/exit points, edge types
2. **Capability Resolution** — Resolves capability references to concrete `CapabilityInstance` bindings
3. **Budget Optimization** — Applies resource budgets from policy
4. **Node Fusion** — Merges compatible sequential nodes
5. **Retry & Fallback Insertion** — Wraps nodes with retry/fallback logic from policy
6. **Scheduling Hints** — Annotates nodes with scheduling metadata
7. **Graph Verification** — Validates final ExecutionGraph invariants

## Strategy Types

| Strategy | Description | Source |
|----------|-------------|--------|
| Single | Single LLM call | `src/strategies/single.rs` |
| Consensus | Multiple independent calls, vote | `src/strategies/consensus.rs` |
| Reflection | Generate → critique → refine loop | `src/strategies/reflection.rs` |
| Debate | Multi-agent debate framework | `src/strategies/debate.rs` |
| ReAct | Reasoning + Action loop | `src/strategies/react.rs` |
| Chain | Sequential chained calls | `src/strategies/chain.rs` |
| Fusion | Parallel calls with synthesis | `src/strategies/fusion.rs` |

## Architectural Invariants (16 Core)

From `docs/architecture/invariants.md`:

1. LLM interactions go through the `Provider` trait
2. Compiler is a series of pure passes — no side effects, no LLM calls
3. Planner produces `WorkflowIR`, not `ExecutionGraph`
4. Scheduler is topology-driven from the `ExecutionGraph`
5. All subgraphs have exactly one entry and one exit point
6. Capability resolution is late-bound at compilation time
7. Resource cleanup is guaranteed through RAII (`ResourceGuard`)
8. Evidence is written after execution, not during
9. Configuration is validated at startup, immutable at runtime
10. Telemetry never blocks request processing
11. Plugin discovery happens at startup only
12. The `ExecutionGraph` is frozen after compilation — never mutated by runtime
13. Compilation is deterministic: same input → same `ExecutionGraph`
14. The compiler pipeline is extensible via registered passes
15. The compiler owns `ExecutionGraph` and is responsible for its lifecycle
16. ADR-027 compiler phase invariants matrix: each phase has "May Do" / "Must Not Do" rules
17. All compiler construction (chat path, execution plane, tests) goes through `build_compiler()`; no production code builds `DefaultCompiler { passes: vec![] }` (ADR-034, v0.13.1)
18. `AppState` and the execution plane (`build_execution_plane`) share the same compiler pipeline and resource manager; deny policy blocks compilation before any graph exists (ADR-034, v0.13.1)

## Feature Flags

| Feature | Default | Gates |
|---------|---------|-------|
| `semantic-cache` | Yes | USearch HNSW semantic vector cache |
| `wasm-plugins` | No | Wasmtime WASM runtime |
| `dev-console` | No | `console-subscriber` for tokio console |
| `otel` | No | OpenTelemetry OTLP exporter |

## Security Mitigations (verified, do not reflag)

These mitigations are already in place. Future audits should treat them as resolved.

| Threat | Mitigation | Location |
|--------|-----------|----------|
| Auth bypass on `/v1/executions` | All routes merged **before** `.layer(auth_middleware)` is applied; axum layers apply to all merged routes | `src/main.rs` (~line 223-236) |
| Auth config missing → open access | `auth_middleware` **fails closed** (returns 401 if `AuthConfig` extension absent) | `src/middleware/auth.rs` (~line 15) |
| Shell interpreter escape (`cmd /c ...`) | Hard blocklist `REJECTED_SHELLS` (`cmd`, `cmd.exe`, `sh`, `bash`, `powershell`, `powershell.exe`, `pwsh`, `zsh`) checked **before** allow-list; reject `cmd` also removed from `config/default.yaml` | `src/tools/shell_tool.rs` (~line 28) |
| Rate limiter busy-loop when `cleanup_interval_secs = 0` | `cleanup_interval_secs.max(1)` clamps to minimum 1 s; config validation rejects 0 at startup | `src/middleware/rate_limit.rs` (~line 46), `src/config/mod.rs` (~line 351) |
| Silent LLM response truncation (`finish_reason="length"` + empty content) | `ensure_non_truncated` returns an error instead of silently returning `""` | `src/providers/mod.rs` (~line 88), used in `zen_model.rs`, `openrouter_model.rs` |

## External Crates (FusionRouter SDK)

| Crate | Path | Purpose |
|-------|------|---------|
| `fusion-plugin-api` | `crates/fusion-plugin-api/` | Minimal SDK for capability plugins |
| `fusion-capability-macros` | `crates/fusion-capability-macros/` | Proc-macros for capability declarations |
| `fusion-capability-sdk` | `crates/fusion-capability-sdk/` | Developer SDK for building capabilities |
