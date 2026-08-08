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
    │  Pass pipeline (4 mandatory passes + optional policy pass)
    │  lower_to_graph: direct structural lowering
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
| `PrimitiveGraph` | Compiler-internal | Lowered IR produced by `Strategy::lower`; expanded into an `ExecutionSubgraph` per node by `strategy_expansion` during compilation |
| `ExecutionGraph` | After Compilation | Executable form: 10 node kinds, resolved models, retry policies, deterministic UUIDs |
| `ExecutionInstance` | During Execution | Bound node + runtime params |
| `ExecutionResult` | After Execution | Standardized output + metrics |

## Compiler Pipeline (build_compiler)

Constructed exclusively via `build_compiler` (ADR-034) — no production path builds `DefaultCompiler` with an empty pass list. One mandatory pass order, plus an optional policy pass appended when a `PolicyIR` is supplied:

1. **Constraint Validation** — Rejects empty IR (at least one node required)
2. **Control Flow Validation** — Validates edge targets, Conditional/Loop/Split/Join/Barrier node shapes, and acyclicity (loop back-edges exempt)
3. **Model Resolution** — Fills `model: None` from the `ModelCatalog` based on requirements (tools → code model, high reasoning → architecture model, else fast)
4. **Budget Optimisation** — Checks `ResourceManager::can_afford`; rejects when the estimated budget is exceeded
5. **Policy Compiler Pass** *(optional)* — Applies a compiled `PolicyIR`; any matched Deny rule blocks compilation

`lower_to_graph` (`src/compiler/mod.rs`) then performs a direct structural lowering of `WorkflowIR` → `ExecutionGraph` (1:1 node-kind mapping; strategy carried as a field on each node; `primitive_graph_hash` is set to 0 — no PrimitiveGraph is produced on the live path). A final compile step (`src/compiler/strategy_expansion.rs`) pre-builds each strategy node's `ExecutionSubgraph` and attaches it to `node.subgraph`; the executor consumes it (runtime `resolve_strategy` lowering remains only as a legacy fallback). **Sub-node message inheritance:** prebuilt subgraphs are built without the request context, so the pipeline (`CompilationStep`, `src/server/pipeline.rs`) injects the assembled `messages` into every LLM sub-node (consensus members, judge, etc.) and the executor re-applies `propagate_parent_messages` as a belt-and-suspenders fallback for both prebuilt and runtime-lowered subgraphs. Each request is re-sent per LLM node, so the per-request `BudgetEnvelope` is sized as `max(quota/5, (input + output) x LLM node count)` with `max_iterations` 100.

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
19. The frozen v0.13 contracts connect to the live path only through deterministic adapters: `intent::lowering::intent_to_workflow` (NormalizedIntent → fusion_ir WorkflowIR), `ir::adapter::workflow_to_types` (→ v0.12 WorkflowIR), `abi::from_graph` (ExecutionGraph → ExecutionAbi, provider-free), `abi::to_graph` (ABI → graph with runtime-supplied model binding), and `eri::local_runtime::LocalEri` (contract 5 over DefaultScheduler/DefaultExecutor). The runtime never interprets intent; it executes ABIs (contract 5).

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
| Insecure defaults (C1) | Defaults are fail-closed: bind `127.0.0.1`, auth on, rate limiting on, CORS same-origin, shell/HTTP tools off; `validate_with_profile(true)` rejects insecure combos; `--unsafe-dev` is the only escape hatch (ADR-035, v0.13.1) | `src/config/mod.rs`, `config/default.yaml`, `src/main.rs` |
| Rate-limit bucket spoofing / starvation (M2) | Bucket key comes from authenticated identity (`ClientIdentity`) or TCP peer address (`ConnectInfo`) — never `x-forwarded-for`; bucket map capped at 100k (new clients denied at cap); limiter layered inside auth (ADR-035, v0.13.1) | `src/middleware/rate_limit.rs`, `src/middleware/auth.rs`, `src/main.rs` |
| Timing side-channel on API keys (M3) | Constant-time comparison over SHA-256 digests (`subtle::ConstantTimeEq`); keys > 1 KB rejected before hashing (ADR-035, v0.13.1) | `src/middleware/auth.rs` |
| Prompt-injection → tool execution (H2, chain root) | Free-form JSON tool parsing removed from executor; execution fed only from provider-native `tool_calls`; `tools.allow_auto_exec` (default false) + non-empty per-request `tool_allowlist` required; provider gets `tools` definitions only when execution is enabled (Law 7 / ADR-037) | `src/executor/mod.rs`, `src/providers/*_model.rs`, `src/config/mod.rs` |
| Shell arg traversal / symlink escape (C3) | Per-command argument policy: known file-reading commands canonicalize every non-flag arg inside `allowed_read_directories` (Law 10 helper `src/security/paths.rs`); `allow_unrestricted_args` default false; `shell_timeout_secs` enforced via tokio timeout (WP 3.2, v0.13.1) | `src/tools/shell_tool.rs`, `src/security/paths.rs`, `src/config/mod.rs` |
| SSRF via HTTP tool (H1) | Scheme allowlist (https default), loopback/link-local/private/IPv4-mapped blocks with DNS resolve-then-recheck (rebinding), `redirect::Policy::none()`, 1 MB body cap + `truncated` flag, `Authorization`/`Host` header overrides dropped, 30 s timeout (WP 3.3, v0.13.1) | `src/tools/http_tool.rs` |

## External Crates (FusionRouter SDK)

| Crate | Path | Purpose |
|-------|------|---------|
| `fusion-plugin-api` | `crates/fusion-plugin-api/` | Minimal SDK for capability plugins |
| `fusion-capability-macros` | `crates/fusion-capability-macros/` | Proc-macros for capability declarations |
| `fusion-capability-sdk` | `crates/fusion-capability-sdk/` | Developer SDK for building capabilities |
