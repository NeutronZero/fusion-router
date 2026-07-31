# FusionRouter Overview

**Version:** 0.12.0
**License:** MIT OR Apache-2.0
**Language:** Rust (2021 edition)

## Project Vision

FusionRouter is an **AI Execution Compiler and Runtime** — an LLM orchestration operating system, event-driven runtime, and release-governed capability platform. It compiles high-level intents into executable DAGs and manages their full lifecycle.

## System Pipeline

```
Request → Context Assembler → Requirements Extractor → Planner → Compiler → Scheduler → Executor → Providers
```

### Pipeline Stages

1. **Server** — Axum HTTP server receives requests, applies middleware (auth, CORS, rate limiting)
2. **Context Assembly** — `ContextAssembler` constructs `ContextSnapshot` with system prompt templates, conversation history, tool definitions
3. **Requirements Extraction** — Classifies request complexity, extracts execution requirements
4. **Planner** — Produces `WorkflowIR` (high-level abstract plan). Three implementations: `SimplePlanner` (single step), `WorkflowPlanner` (registry-first), `DynamicPlanner` (LLM-generated with safety guards)
5. **Compiler** — Pure deterministic pass pipeline: lowers `WorkflowIR` → `PrimitiveGraph` → `ExecutionGraph`. 7 standard passes with optimization framework.
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
| `compiler` | `src/compiler/` | Compiler passes, IR (PrimitiveGraph, StrategyIR), optimization |
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
| `connectors` | `src/connectors/` | GitHub, Browser, MCP, Filesystem, HTTP, Shell |
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
10. `ExecutionGraph` is derived from `PrimitiveGraph`

See `.memory/architecture.md` for the full invariant set.
