# Architecture Decision Records Index

## ADR Directory: `docs/adr/`

| # | Title | Status | Summary |
|---|-------|--------|---------|
| 001 | Foundation | Accepted | HTTP-first, trait-driven, async-native foundation with Provider abstraction |
| 002 | Planner | Accepted | Planner produces WorkflowIR (not ExecutionGraph), policy-driven, evidence-informed |
| 003 | Compiler | Accepted | Deterministic pure pass pipeline. Lowering produces ExecutionGraph |
| 004 | Scheduler | Accepted | Topology-driven work queue, state machine, retry/fallback |
| 005 | Provider Abstraction | Accepted | Three-part split: Model + Transport + Provider |
| 006 | DAG Support | Accepted | Conditional, Loop, Split, Join, Barrier node kinds |
| 007 | Error Handling | Accepted | Centralized `RouterError`, `PipelineStage` enum, HTTP mapping |
| 008 | Telemetry & Observability | Accepted | Tracing, OTel (gated), Prometheus, AuditLog, SQLite evidence |
| 009 | Configuration Management | Accepted | YAML config, env var overlay, startup validation (11 checks) |
| 010 | Plugin System | Accepted | 4 extension points, TOML manifest, WASM (gated), no hot-reload |
| 011 | Testing Strategy | Accepted | Inline unit tests, mock providers, async support, 15+ categories |
| 012 | Security Model | Accepted — Amended by ADR-035 | API key auth, CORS, token bucket rate limiting; fail-closed post-ADR-035 |
| 013 | Workflow Registry | Accepted | WorkflowDefinition YAML schema, template instantiation to WorkflowIR |
| 014 | Workflow Planner | Accepted | Registry-first, SimplePlanner fallback |
| 015 | Dynamic Workflow | Accepted | LLM-generated WorkflowIR, static/dynamic/hybrid modes, safety guards |
| 016 | Intent-Oriented Execution | Approved | Clients express intent/mode/constraints, not mechanics |
| 017 | Execution Runtime ABI | Accepted | ExecutionResult formal ABI, single scheduler loop, compiler owns graphs |
| 018 | Strategy SDK | Proposed | Two-layer IR (StrategyIR, PrimitiveIR), deterministic replay |
| 019 | Primitive Execution Graph Alignment | Accepted | PrimitiveGraph canonical; ExecutionGraph derived via `to_execution_graph()` — used for strategy expansion (executor-side today, not a compiler stage) |
| 020 | Compiler Optimization Framework | Accepted | 6-pass taxonomy, 7 selection criteria, legality rules, rollback safety |
| 021 | Capability Platform | Proposed | Immutable CapabilityRegistry, unified execution, freeze at startup |
| 022 | Plugin ABI | Proposed | Version negotiation, metadata/execution separation |
| 023 | Capability Resolution | Proposed | Dedicated CapabilityResolver, CapabilityGraph, LRU cache |
| 024 | Policy Compilation | Proposed | Declarative policy compilation via compiler pass, NodeMetadata annotations |
| 025 | Connector Abstraction | Proposed | Planner agnosticism, late binding via ConnectorResolver |
| 026 | Execution Session | Proposed | ExecutionSession/SessionStore decoupling, multiple backends |
| 027 | Compiler Phase Invariants | Accepted | "May Do" / "Must Not Do" matrix for each compiler phase |
| 028 | Capability Contract Evolution | Proposed | SemVer for contracts, aliasing/deprecation, feature flags |
| 029 | Execution Semantics | Accepted (Frozen) | Formal state machine, event emission, cancellation/timeout/idempotency |
| 030 | Session Replay Semantics | Accepted (Frozen) | 3 replay modes, compatibility validation on resume |
| 031 | Trigger Request Semantics | Accepted (Frozen) | Canonical ExecutionRequest, single-pipeline, payload immutability |

## ADR Directory: `docs/adrs/`

| # | Title | Status | Summary |
|---|-------|--------|---------|
| ADR-017 (Runtime Event Stream ABI) | Runtime Event Stream ABI | Approved (docs/adrs/) | Event sourcing substrate, immutable events, EventBus/ProjectionDispatcher |
| ADR-018 | Capability Binary Interface | Approved | `.fusionpkg` format, typed permissions, WASI sandbox invariants |
| ADR-019 | Capability Host Interface | Approved | `CapabilityHostServices` trait, 5 host functions, SandboxRuntime |
| ADR-032 | Execution ABI Separate from PrimitiveGraph | Accepted | PrimitiveGraph stays compiler-internal; ABI generator emits ExecutionAbi (v0.13 contract, Decision 1) |
| ADR-033 | v0.13 Architecture Freeze | Accepted (Frozen) | Six core abstractions frozen as stable public contracts |
| ADR-034 | Single Compiler Pipeline | Draft | `build_compiler()` sole construction path; deny = compile error; total capability policy (v0.13.1 charter) |
| ADR-035 | Fail-Closed Deployment | Draft | Fail-closed defaults; `--unsafe-dev`; identity-based rate limiting; constant-time key check (v0.13.1 charter) |
| ADR-036 | Plugin Execution Context | Draft | `PluginExecutionContext`; caller-bound permissions; metered/timed WASM; content-bound attestation (v0.13.1 charter) |
| ADR-037 | Structured Tool Invocation | Draft | Provider-native `tool_calls` only; model output is never executable (v0.13.1 charter) |
| ADR-038 | BudgetOptimisationPass Port | Accepted | 7-method trait in kernel, state-aware stub in compiler, real instance stays in monolith; accumulation test pattern |

## ADR Location: `docs/decisions/`

| File | Title | Summary |
|------|-------|---------|
| `provenance-schema.md` | Provenance Schema | Trace/evidence data model |
| `resource-guard-contract.md` | Resource Guard Contract | RAII resource cleanup contract |

## Status Legend

| Status | Meaning |
|--------|---------|
| Accepted | Design decision made and implemented |
| Accepted (Frozen) | Final, no further changes expected |
| Proposed | Under consideration, not yet implemented |
| Approved | Approved (some in separate tracking) |
