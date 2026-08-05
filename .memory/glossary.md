# FusionRouter Glossary

## A

**ADR** — Architecture Decision Record. Canonical documents in `docs/adr/` and `docs/adrs/` recording architectural decisions and their rationale.

**Attestation** — Cryptographically signed evidence that a release gate has passed, with 4-phase verification (Schema → Canonical → Signature → Semantic).

## C

**Capability** — A versioned, permission-scoped, reusable execution unit. The unified abstraction replacing ad-hoc plugins/tools/connectors.

**CapabilityId** — Strongly-typed identifier for a capability (e.g., `echo.text`).

**CapabilityContract** — Declarative ABI specifying capability identity, version, I/O schemas, permissions, dependencies, and quality attributes.

**CapabilityInstance** — A bound capability execution object pairing a contract with runtime parameters.

**CircuitBreaker** — 3-state (Closed/Open/Half-Open) failure isolation mechanism for provider calls.

**ClientIdentity** — Authenticated client identifier set by the auth middleware (SHA-256 of the API key) and used by the rate limiter for bucket keying.

**Compiler** — Pure deterministic pass pipeline constructed via `build_compiler` that lowers `WorkflowIR` → `ExecutionGraph` through `lower_to_graph`.

**Compiler Pass** — A single pure transformation or validation step in the compiler pipeline.

**Connector** — An adapter for external services (GitHub, Browser, MCP, Filesystem, HTTP, Shell). Filesystem/HTTP/GitHub perform real work; Browser/MCP/Shell are honest stubs that fail closed with a "not implemented" error.

**ContextAssembler** — Pipeline stage that builds `ContextSnapshot` from system prompt templates, conversation history, and tool definitions.

**ContextSnapshot** — The assembled context including system prompt, conversation history, and available tools.

## F

**Fail-closed** — Default posture (ADR-035): a default install is unreachable without authentication, bound to `127.0.0.1`, rate-limited, CORS same-origin, with shell/HTTP tools disabled; release-mode `validate()` rejects any insecure combination unless `--unsafe-dev` is passed.

## D

**DynamicPlanner** — LLM-powered planner that generates `WorkflowIR` from intent, with configurable modes (Static/Dynamic/Hybrid) and safety guards.

## E

**EventBus** — Trait for the runtime event stream, with `BroadcastEventBus` as the in-memory implementation.

**ExecutionGraph** — The final compiled output: 12 concrete node kinds with resolved models, retry policies, and deterministic UUIDs.

**ExecutionResult** — Standardized execution output containing result values and metrics.

**ExecutionSession** — Identity container for a single execution run, enabling replay and continuity.

## F

**FeedbackCalibrator** — Closed-loop calibration using exponential moving average (EMA α=0.2, cold-start n≥30).

## G

**GraphVisualizer** — DevEx tool for visualizing `ExecutionGraph` structure.

## I

**IntentPlanner** — Entry-point planner that delegates to the appropriate sub-planner based on intent classification.

## L

**Law 7 (Runtime)** — "Tool execution is fed only from provider-native `tool_calls`, never from model output text." Executor parses no free-form JSON for tool invocation; a model printing `{"tool": ...}` yields text only (ADR-037).

**LifecycleManager** — Orchestrates session lifecycle: creation, checkpointing, suspension, resumption, teardown.

## M

**Model** — Trait for LLM-specific behavior (prompt formatting, response parsing).

## N

**native_tool_calls** — Typed tool-call field on `ChatCompletionResponse` (`Option<Vec<ToolCall>>`), normalized from provider wire shape (`choices[0].message.tool_calls` or Ollama `message.tool_calls`) by `native_tool_calls_from`; the only source of tool execution (Law 7).

## P

**PassManager** — Helper for composing compiler pass lists (`src/compiler/passes/mod.rs`); the production pipeline is the fixed `build_compiler` pass list.

**Pipeline** — The 8-stage request processing pipeline: Server → Context → Requirements → Planner → Compiler → Scheduler → Executor → Providers.

**Planner** — Pipeline stage that converts intent + context into `WorkflowIR`.

**Plugin** — Extensions packaged as native Rust crates or WASM modules (`.fusionpkg`).

**Policy** — Declarative rules that influence compilation (retry, timeout, budget) and release governance (gates, environments).

**PrimitiveGraph** — Compiler-internal IR produced by `Strategy::lower`; used for per-node strategy expansion, materialized into `ExecutionSubgraph`s by the compiler's `strategy_expansion` at compile time.

**ProjectionDispatcher** — Decouples event production from consumption in the event system.

**Provider** — Trait composing `Model` + `Transport` for unified LLM access.

**ProviderRegistry** — Registry of available providers for request routing.

**ProviderRouter** — Routes LLM requests across available providers.

## R

**ReplayEngine** — Enables Deterministic, Inspection, and Simulation replay modes from session snapshots.

**RequirementsExtractor** — Pipeline stage that classifies request complexity and extracts execution requirements.

**ResourceGuard** — RAII guard ensuring resource cleanup.

## S

**Scheduler** — Topology-driven DAG executor using `WorkQueue` state machine.

**SessionSnapshot** — Point-in-time capture of execution state for replay.

**SessionStore** — Trait for session storage (Memory, SQLite implementations).

**SimplePlanner** — Default fallback planner producing single-node `WorkflowIR`.

**Strategy** — Multi-step reasoning pattern (Single, Consensus, Reflection, Debate, ReAct, Chain, Fusion).

**StrategyIR** — Strategy-aware IR in the two-layer compiler model.

## T

**ToolCall** — `{ id, name, arguments: Value }` structured tool invocation bound to the model response by the provider transport; the only source of tool execution (ADR-037).

**TraceInspector** — DevEx tool for execution trace inspection and debugging.

**Transport** — Wire protocol abstraction (HTTP, WebSocket, Stdio).

**Trigger** — Execution initiation mechanism (Webhook, Cron, EventBus).

**TriggerTrace** — Provenance chain for triggered executions.

## U

**unsafe-dev** — CLI flag (`--unsafe-dev`, `AppConfig::unsafe_dev`) that explicitly disables fail-closed deployment posture (auth off, rate limit off, wildcard CORS, permissive tools, placeholder API keys). Debug/development escape hatch only; never for production (ADR-035).

## W

**WorkflowDefinition** — YAML-declared workflow template matched against requirements.

**WorkflowIR** — High-level abstract plan output by the Planner, with semantic node types.

**WorkflowPlanner** — Registry-first planner that matches requirements to `WorkflowDefinition`.

**WorkflowRegistry** — Loads and caches YAML workflow definitions.

**WorkQueue** — Topological DAG scheduling queue tracking node dependencies.
