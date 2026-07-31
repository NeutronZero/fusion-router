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

**Compiler** — Pure deterministic pass pipeline that lowers `WorkflowIR` → `PrimitiveGraph` → `ExecutionGraph`.

**Compiler Pass** — A single pure transformation or validation step in the compiler pipeline.

**Connector** — An adapter for external services (GitHub, Browser, MCP, Filesystem, HTTP, Shell).

**ContextAssembler** — Pipeline stage that builds `ContextSnapshot` from system prompt templates, conversation history, and tool definitions.

**ContextSnapshot** — The assembled context including system prompt, conversation history, and available tools.

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

**LifecycleManager** — Orchestrates session lifecycle: creation, checkpointing, suspension, resumption, teardown.

## M

**Model** — Trait for LLM-specific behavior (prompt formatting, response parsing).

## P

**PassRegistry** — Registry of available compiler passes, supporting plugin extensions.

**Pipeline** — The 8-stage request processing pipeline: Server → Context → Requirements → Planner → Compiler → Scheduler → Executor → Providers.

**Planner** — Pipeline stage that converts intent + context into `WorkflowIR`.

**Plugin** — Extensions packaged as native Rust crates or WASM modules (`.fusionpkg`).

**Policy** — Declarative rules that influence compilation (retry, timeout, budget) and release governance (gates, environments).

**PrimitiveGraph** — The canonical lowered IR form, strategy-expanded, from which `ExecutionGraph` is derived.

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

**TraceInspector** — DevEx tool for execution trace inspection and debugging.

**Transport** — Wire protocol abstraction (HTTP, WebSocket, Stdio).

**Trigger** — Execution initiation mechanism (Webhook, Cron, EventBus).

**TriggerTrace** — Provenance chain for triggered executions.

## W

**WorkflowDefinition** — YAML-declared workflow template matched against requirements.

**WorkflowIR** — High-level abstract plan output by the Planner, with semantic node types.

**WorkflowPlanner** — Registry-first planner that matches requirements to `WorkflowDefinition`.

**WorkflowRegistry** — Loads and caches YAML workflow definitions.

**WorkQueue** — Topological DAG scheduling queue tracking node dependencies.
