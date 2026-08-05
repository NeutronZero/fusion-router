# FusionRouter Overview

**Version:** 0.13.1
**License:** MIT OR Apache-2.0
**Language:** Rust (2021 edition)

## Project Vision

FusionRouter is an **AI Execution Compiler and Runtime** — an LLM orchestration operating system, event-driven runtime, and release-governed capability platform. It compiles high-level intents into executable DAGs and manages their full lifecycle.

## Frozen v0.13 Architecture

The v0.13.0 architecture is frozen. Six stable contracts define the system:

| Contract | Location |
|----------|----------|
| NormalizedIntent | `src/intent/` |
| WorkflowIR | `src/ir/` |
| ExecutionAbi | `src/abi/` |
| ExecutionTarget | `src/target/` |
| ExecutionRuntimeInterface | `src/eri/` |
| CapabilityRegistry + CapabilityTrait | `src/capability/`, `crates/fusion-plugin-api` |

All new development integrates through these contracts (ADR-033). The v0.12
planner→compiler→scheduler→executor pipeline remains functional and is now
**bridged** to the contracts by deterministic adapters:
`intent::lowering::intent_to_workflow` → `ir::adapter::workflow_to_types`
→ `build_compiler` → `abi::from_graph::abi_from_graph` (ABI generator)
→ `eri::local_runtime::LocalEri` (contract 5 runtime over the live engine,
with `abi::to_graph::graph_from_abi` binding runtime models). Full reconcile
beyond those bridges remains v0.14 boundary work.

## Request Paths (dual stack, by design)

| Path | Binary | Status |
|------|--------|--------|
| Production monolith | `fusion-router` (`src/`, v0.13.x) | **Live request path**: Axum server → planner → `build_compiler` → `lower_to_graph` → scheduler → executor |
| Studio sandbox | `fusion-server` (`apps/`, v0.14.x) | **Simulation-only**: serves the Studio UI from `fusion-studio-api`; the `fusion-*` workspace crates return placeholder data. Every API response carries `"simulation": true`. Never wired into the monolith. Default port 8787 (`FUSION_STUDIO_PORT` overridable) — distinct from the monolith's 8080 |

There is no feature flag switching between them: they are separate binaries.
Wiring the Studio crates to the real runtime is v0.14 boundary work; until then
they are UI-development sandboxes, not a second production stack.

## System Pipeline

```
Request → Context Assembler → Requirements Extractor → Planner → Compiler → Scheduler → Executor → Providers
```

### Pipeline Stages

1. **Server** — Axum HTTP server receives requests, applies middleware (auth, CORS, rate limiting)
2. **Context Assembly** — `ContextAssembler` constructs `ContextSnapshot` with system prompt templates, conversation history, tool definitions
3. **Requirements Extraction** — Classifies request complexity, extracts execution requirements
4. **Planner** — Produces `WorkflowIR` (high-level abstract plan). Three implementations: `SimplePlanner` (single step), `WorkflowPlanner` (registry-first), `DynamicPlanner` (LLM-generated with safety guards)
5. **Compiler** — Pure deterministic pass pipeline constructed via `build_compiler` (ADR-034): 4 mandatory passes (ConstraintValidation, ControlFlowValidation, ModelResolution, BudgetOptimisation) plus an optional PolicyCompilerPass, then a direct structural `lower_to_graph` (`WorkflowIR` → `ExecutionGraph`). Strategy expansion into per-node `ExecutionSubgraph`s happens at compile time (`strategy_expansion`), with runtime lowering kept only as a legacy fallback.
6. **Scheduler** — Topology-driven DAG scheduler using `WorkQueue`. Manages node state machine: Pending → Running → Succeeded/Failed. Handles Conditional, Loop, Split, Join, Barrier nodes.
7. **Executor** — Dispatches scheduled nodes to providers/strategies/tools/connectors. `CapabilityExecutor` for unified capability execution.
8. **Providers** — Model/Transport abstraction layer. Supports OpenRouter, Zen, Ollama.

## Major Modules

| Module | Source | Purpose |
|--------|--------|---------|
| `server` | `src/server/` | Axum HTTP server, pipeline, handlers, health checks |
| `context` | `src/context/` | Context assembly, trimming, token estimation |
| `requirements` | `src/requirements/` | Intent classification, complexity extraction |
| `planner` | `src/planner/` | IntentPlanner, SimplePlanner, DynamicPlanner, WorkflowPlanner |
| `compiler` | `src/compiler/` | Pass pipeline (4 mandatory + optional policy), `lower_to_graph`, IR types |
| `scheduler` | `src/scheduler/` | DefaultScheduler, WorkQueue, DistributedScheduler, ConnectorResolver |
| `executor` | `src/executor/` | DefaultExecutor, CapabilityExecutor |
| `strategies` | `src/strategies/` | Single, Consensus, Reflection, Debate, ReAct, Chain, Fusion |
| `providers` | `src/providers/` | ProviderRouter, ProviderRegistry, CircuitBreaker, model adapters |
| `transport` | `src/transport/` | HTTP, WebSocket, Stdio transports with exponential backoff |
| `capability` | `src/capability/` | CapabilityRegistry (mutable-then-frozen), permissions |
| `policy` | `src/policy/` | Policy compilation: AST, IR, precedence engine, traces |
| `session` | `src/session/` | ExecutionSession, SessionSnapshot, CheckpointEngine, ReplayEngine |
| `lifecycle` | `src/lifecycle/` | Session lifecycle orchestration |
| `trigger` | `src/trigger/` | ExecutionRequest, Webhook, Cron, EventBus handlers |
| `connectors` | `src/connectors/` | GitHub, Browser, MCP, Filesystem, HTTP, Shell — honest implementations: filesystem/http/github perform real I/O (github requires `GITHUB_TOKEN`); browser/mcp/shell fail closed with "not implemented" errors instead of fabricating results |
| `workflow` | `src/workflow/` | WorkflowRegistry: YAML workflow loading |
| `tools` | `src/tools/` | Tool trait, ToolRegistry, built-in tools |
| `telemetry` | `src/telemetry/` | EvidenceRepository, SqliteEvidenceRepository, FeedbackCalibrator, FusionMetrics, AuditLog |
| `events` | `src/events/` | Event stream ABI, ExecutionEventEnvelope, BroadcastEventBus, ProjectionDispatcher |
| `release` | `src/release/` | Release governance: gates, policy, attestation, signing, archive |
| `plugin` | `src/plugin/` | PluginManager, PluginManifest, WASM loading |
| `cache` | `src/cache/` | Semantic cache (feature-gated: `semantic-cache`) |
| `middleware` | `src/middleware/` | Auth (API key), CORS, rate limiter, request ID |
| `config` | `src/config/` | AppConfig with YAML deserialization/validation |
| `resource` | `src/resource/` | ResourceManager, ResourceGuard (RAII), BudgetEnvelope |
| `devex` | `src/devex/` | GraphVisualizer, TraceInspector, PluginScaffolder |
| `feature_gate` | `src/feature_gate/` | FeatureRegistry, FeatureFlag, FeatureState |
| `wasm` | `src/wasm/` | Wasmtime runtime, fuel metering (feature-gated: `wasm-plugins`) |

## Coding Rules

- Zero warnings on build
- Dead code annotated with `#[allow(dead_code)]` + comment
- Atomic commits with conventional commit messages (`feat:`, `fix:`, `chore:`)
- Public API stability: no signature changes without approval
- Heavy deps gated behind feature flags

## Core Architectural Invariants

1. Planner produces `WorkflowIR`, not `ExecutionGraph`
2. Compiler is a pipeline of pure, deterministic passes
3. Compiler owns the frozen `ExecutionGraph` until retirement
4. Scheduler is topology-driven (work queue)
5. Scheduler owns output selection
6. Executor dispatches via `CapabilityExecutor`
7. LLM interactions go through `Provider` trait
8. All subgraphs have exactly one entry and one exit point
9. Capability resolution is late-bound
10. `ExecutionGraph` is produced by the `lower_to_graph` structural lowering; `PrimitiveGraph` is a compiler-internal IR used for strategy expansion

See `.memory/architecture.md` for the full invariant set.
