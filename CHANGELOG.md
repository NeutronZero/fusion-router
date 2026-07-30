# Changelog

## [Unreleased]

- **Capability Resolution Bridge (Sprint O2.5)** (`src/planner/resolver/capability/`)
  - SemVer resolution — extended `RequirementSet` with version constraints, resolver selects best compatible via `semver::VersionReq`
  - Dependency expansion — BFS transitive expansion of declared dependencies before graph construction
  - Policy constraints — allow/deny lists and release profile filtering evaluated during resolution, not in registry or runtime
  - `CapabilityGraphLowerer` — deterministic lowering from `CapabilityGraph` to compiler `ExecutionGraph` via dedicated transformation component
  - `ResolverError` — typed error enum replacing `Result<_, String>` across the resolution pipeline
  - Resolver determinism guarantee — identical registry + requirements + policy → identical graphs
  - All existing `CapabilityGraph`, `CapabilityResolver`, and registry invariants preserved

- **Typed Permissions & Capability Registry (Sprint O2)** (`crates/fusion-plugin-api/`, `src/capability/`)
  - **`Permission` enum** — typed ABI enum (`Network`, `Filesystem`, `Http`, `Secrets`, `Environment`) with `validate()`, `Display`, `FromStr`, `Serialize`, `Deserialize`, and full trait support (`Debug`, `Clone`, `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`)
  - **`PermissionError`** — `thiserror`-derived error type for `Permission::validate()`
  - **ABI version bump** — `CAPABILITY_ABI_VERSION` updated from `"0.1.0"` to `"0.2.0"`
  - **`CapabilityContract.permissions`** — changed from `Vec<String>` to `Vec<Permission>`, all consumers migrated
  - **`CapabilityRegistry` trait** — `register()`, `get()`, `contains()`, `list()`, `freeze()`, `is_frozen()` with `Send + Sync` bound
  - **`InMemoryCapabilityRegistry`** — `HashMap`-backed implementation with non-consuming `freeze()`, sorted `list()`, and permission validation during `register()`
  - **`RegistryError`** — typed `#[non_exhaustive]` enum with `DuplicateId`, `Frozen`, `NotFound`, `InvalidContract` variants
  - **`CapabilityDescriptor`** — discovery metadata wrapping `CapabilityContract` with `tags`, `categories`, `discoverable`, `provider`, `source`
  - **`CapabilitySource`** — enum (`Builtin`, `Package`, `Development`, `Remote`) with serialization support
  - **Macro & SDK migration** — `PermissionAttr::to_permission_token_stream()` emits typed `Permission` values; `CapabilityBuilder::permission()` accepts `Permission`; SDK prelude re-exports `Permission`
  - **Consumer migration** — `PluginManager`, `CapabilityResolver`, `phase_invariants.rs`, 6 connectors updated to typed permissions and trait-based registry
  - **Validation pipeline** — 3-stage validation (builders → registry `register()` → runtime assumes valid)

- **Capability Platform SDK (Sprint O1)** (`crates/fusion-capability-sdk/`, `crates/fusion-capability-macros/`)
  - **`fusion-capability-macros`** — proc-macro crate providing `#[capability]` attribute macro and `#[permission]` helper attribute
  - **`#[capability]` macro** — generates `Plugin` and `CapabilityPlugin` trait implementations from annotated structs with compile-time semver validation and permission parsing
  - **`#[permission]` attribute** — typed permission declarations (`Network`, `Filesystem`, `Http`, `Secrets`) with compile-time error propagation
  - **`fusion-capability-sdk`** — developer SDK crate with `CapabilityBuilder`, `CapabilityManifestBuilder`, `SchemaBuilder`, and intentionally small `prelude`
  - **`CapabilityBuilder`** — fluent, immutable-after-`finish()` builder for constructing `CapabilityContract` values
  - **`CapabilityManifestBuilder`** — ADR-018 manifest stub builder with default `CAPABILITY_ABI_VERSION`
  - **`SchemaBuilder`** — JSON Schema builder with optional `schemars`-based type derivation
  - **`CAPABILITY_ABI_VERSION`** — shared constant `"0.1.0"` added to `fusion-plugin-api`
  - **Architecture:** Three-crate stack (`fusion-plugin-api` → `fusion-capability-macros` → `fusion-capability-sdk`) preserving stable ABI no new execution trait hierarchy
  - **Testing:** 15 unit tests + 5 trybuild compile-fail tests + 4 integration tests + clippy-clean

## [0.11.0] – 2026-07-27

- **Runtime Intelligence & Event-Driven Core (v0.11)** (`src/events/`)
  - **ADR-017 (Runtime Event Stream ABI):** Formalized immutable event stream contract, monotonic sequence numbers, correlation ID propagation, and projection panic isolation guarantees.
  - **Canonical Event Model (`src/events/mod.rs` & `payload.rs`):** `ExecutionEventEnvelope` carrying `schema_version: "fusion.router.event.v1"`, `event_id`, `workflow_id`, `execution_id`, `correlation_id`, `sequence_number`, `timestamp`, `parent_event_id`, and typed `ExecutionEvent` taxonomy.
  - **Abstract EventBus Trait (`src/events/bus.rs`):** `EventBus` trait with `BroadcastEventBus` (backed by `tokio::sync::broadcast`).
  - **Decoupled Projection Framework (`src/events/projection.rs`):** `EventProjection` trait & `ProjectionDispatcher` background fan-out dispatcher with panic isolation.
  - **OpenTelemetry Projection (`src/events/consumers/otel.rs`):** Span & trace rendering projection.
  - **Timeline Visualizer (`src/events/consumers/timeline.rs`):** `TimelineProjection` & `TimelineModel` with millisecond-accurate ASCII and JSON renderers.
  - **Policy-Driven Checkpointing (`src/events/consumers/checkpoint.rs`):** `CheckpointEngine` with `CheckpointPolicy` (`EveryNode`, `EveryNthNode`, `Timed`, `Manual`) and idempotent snapshotting.
  - **Persistent Event Store (`src/events/consumers/storage.rs`):** Append-only JSONL event log store with ordered retrieval.
  - **CLI Tracing Tooling (`src/bin/fusion.rs`):** `fusion trace timeline <EXEC_ID> [--format text|json]` and `fusion trace events <EXEC_ID> [--format text|json]`.
- **Feature Flag Infrastructure** (`src/feature_gate/`)
  - `FeatureFlag` enum (Streaming, Replay, ConnectorHealth, SemanticCache, WasmPlugins) with serde kebab-case
  - `FeatureRegistry` with derived lookup map (no manual match statements), two-phase hot-reload via `ConfigSubscriber`
  - `FeatureGateSubscriber` for live config reload of feature flag overrides
  - `FeatureDefinition`, `FeatureState`, `FeatureConfig`, `Stability` types
- **Release Gate Framework & SemVer Enforcement (Sprint M1)** (`src/release/`)
  - `GateId` strongly-typed enum (`SDK-1`, `RPL-1`, `UPG-1`, `DET-1`, `PLG-1`, `STR-1`, `PRV-1`, `CON-1`) with custom serde and Display
  - `GateCategory`, `GateContext`, `GateResult`, `GateCheck`, `GateMetadata`, `GateError` types
  - `GateExecution` enum distinguishing success from execution errors
  - `ReleaseGate` trait with `Send + Sync` for composable gates
  - `GateRunner` with FIFO execution ordering, `run_all()`/`run_one()` methods
  - `GateReport` with JSON and text output formats
  - `SemVerGate` (`SDK-1`) implementing `ReleaseGate` via `cargo semver-checks`
- **Compatibility & Upgrade Assurance (Sprint M2)** (`src/release/`)
  - Unified `FixtureLoader` & `FixtureManifest` infrastructure preserving entry order
  - `ReplayGate` (`RPL-1`) — historical snapshot replay compatibility
  - `UpgradeGate` (`UPG-1`) — historical configuration parser compatibility
  - `DeterminismGate` (`DET-1`) — planner execution graph determinism validation
- **Ecosystem Certification Gates (Sprint M3)** (`src/release/certification.rs` & `src/release/gates/`)
  - Shared `CertificationArtifact` trait and `CertificationContext`
  - `FixtureKind` extended for `Plugins`, `Strategies`, `Providers`, `Connectors` with stable entry `id` fields
  - `PluginGate` (`PLG-1`) — plugin manifest, SDK semver compatibility, symbol exports, and capabilities
  - `StrategyGate` (`STR-1`) — routing strategy registration, compiler `ExecutionGraph` generation, and policy
  - `ProviderGate` (`PRV-1`) — provider catalog declarations, pricing metadata schema, and auth schema
  - `ConnectorGate` (`CON-1`) — connector protocol schema (`v1`), serialization, and health endpoint declarations
- **Release Policy Engine (Sprint M4)** (`src/release/policy.rs`, `src/release/waiver.rs`, `src/release/evaluator.rs`)
  - Typed `ReleaseEnvironment` enum (`Production`, `Staging`, `Development`, `Custom`)
  - Data-driven `PolicyDefinition` (`policy.yaml`) for environment `require` vs `advisory` gate mapping
  - Scoped, auditable `WaiverSet` (`waivers.yaml`) with mandatory stable IDs (`id: waiver-2026-0042`) and RFC3339 expiration checking
  - Two-phase `PolicyEvaluator` (`EvidenceClassifier` → `WaiverMatcher` → `PolicySummary` → `ReleaseDecision`)
  - Subcommand `fusion gates evaluate --env production` with human-readable summary renderer
- **Evidence Preservation & Release Attestation (Sprint M5)** (`src/release/assessment.rs`, `src/release/attestation.rs`, `src/release/signing.rs`, `src/release/envelope.rs`, `src/release/archive.rs`, `src/release/verifier.rs`)
  - Immutable `ReleaseAssessment` handoff bundle with content-derived `assessment_id` (`asm-<hex>`)
  - `AttestationBuilder` as sole authority for canonical UTF-8 JSON byte serialization
  - `SignatureBlock` (version `1`) and abstract `Signer` trait (`MockSigner`, `Ed25519Signer`)
  - Transport `AttestationEnvelope` wrapping signed attestations
  - Append-only `FilesystemArchiveBackend` storing attestations under `.fusion/attestations/*.json` (rejects overwrites)
  - 4-Phase `AttestationVerifier` pipeline (Schema Validation → Canonical Serialization → Cryptographic Verification → Semantic Consistency)
- **Bootstrap & Wiring** (`src/release/bootstrap.rs`)
  - Single composition point `build_default_runner()` registering all 8 release gates in canonical order
- **CLI Governance Tooling** (`src/bin/fusion.rs`)
  - `fusion gates list` — list registered release gates
  - `fusion gates check [--gate <ID>] [--format json|text]` — run release gates
  - `fusion gates explain <ID>` — show gate metadata
  - `fusion gates evaluate [--env <ENV>] [--policy <PATH>] [--waivers <PATH>]` — evaluate release policy
  - `fusion gates attest [--env <ENV>] [--output-dir <DIR>]` — create & sign release attestation
  - `fusion gates verify-attestation <PATH_OR_ID>` — verify attestation envelope with 4-phase verification report
  - `fusion features list [--format json|text]` — list feature flags with state
  - Clap derive argument parsing with `--help` support
- `AppConfig.features` field + `config/default.yaml` features section for YAML-driven feature configuration
- 5 integration tests covering mock semver (pass/fail), JSON round-trip, FIFO ordering, feature registry

### Changed
- SqliteSessionStore – placeholder stub remains; production SQLite backend deferred to v0.11+

### Removed
- `config/default.yaml` – replaced by structured config crate

## [0.10.0] – 2026-07-26

### Added
- **Trigger Framework & Unified Ingress** (Phase 7A / ADR-031)
  - `ExecutionRequest` canonical ingress contract normalizing all entry points
  - `WebhookTriggerHandler`, `CronTriggerScheduler`, `EventBusTriggerSubscriber` handlers
  - `TriggerTrace` provenance chain with `PolicyTrace` → `ExecutionTrace` layering
  - `TriggerIR` intermediate representation for trigger-to-pipeline bridging
  - `TriggerExecutionEngine` orchestrating trigger dispatch
- **Session Continuity & Replay** (Phase 5 / ADR-030)
  - `ExecutionSession` / `SessionSnapshot` identity-state decoupling
  - 3 replay modes: `Deterministic`, `Inspection` (side-effect free), `Simulation` (mocked)
  - `SessionStore` trait with `InMemorySessionStore` and `SqliteSessionStore` (stub) backends
  - `CheckpointEngine` with atomic checkpoint creation and restore
  - `ResumeEngine` with API version compatibility validation
  - `LifecycleManager` for session lifecycle orchestration
- **Connector Ecosystem** (Track B)
  - `Connector` trait with `ConnectorDescriptor` and `TransportRequest`/`TransportResponse`
  - 6 reference connectors: `GitHubConnector`, `BrowserConnector`, `McpConnector`, `FilesystemConnector`, `HttpConnector`, `ShellConnector`
  - `ConnectorResolver` for late-bound connector dispatch
  - `CapabilityPlugin` integration via `CapabilityContract` on GitHubPlugin
- **Developer Experience & Diagnostics** (Track C)
  - `GraphVisualizer` with Mermaid and ASCII output formats
  - `TraceInspector` with structured level-filtered diagnostics
  - `PluginScaffolder` generating plugin project templates from built-in skeletons
  - `UnifiedDiagnosticsEnvelope` aggregating cross-subsystem diagnostics
- **Distributed Scheduling** (Track D)
  - `DistributedScheduler` implementing `Scheduler` trait with `RemoteWorkerPool` + `DefaultScheduler` local fallback
  - Zero kernel API changes — fully additive
- **Production Hardening** (Track A)
  - Stress test suite: 100 concurrent DAGs, backpressure queue depth limits, fault injection with retry/fallback, session recovery
  - Backpressure via `buffer_unordered(max_concurrent)` (default 16)
- **Capability Resolution** (Phase 2 / ADR-023)
  - `CapabilityGraph` with `CapabilityResolver` as pure compiler step
  - Kahn topological sort cycle detection, conflict checking
- **Declarative Policy Compilation** (Phase 4 / ADR-024)
  - `PolicyAST` → `PolicyIR` → `PolicyCompilerPass` transformation pipeline
  - `PolicyPrecedenceEngine` with Deny > Approval > Allow strict ordering
  - `PolicyTrace` with `PolicyMatchEvent` for audit trail
  - Gate node injection into execution graphs
- **Artifact Model** – `Artifact` trait with `clone_box()`, `version()`, `kind()`, `artifact_type()`; `stored_artifacts` on `ExecutionResult`
- **ExecutionContext & ExecutionTrace** – standardized runtime container with append-only event log and provenance trace
- **Enhanced Transport Layer** – `StdioTransport` (subprocess I/O), `WebSocketTransport`, `Backoff` with full-jitter exponential retry
- **Ollama Model** – `ollama_model.rs` with local inference model definition (32K context, 0.85 coding)

### Changed
- **Architecture Document** – comprehensive update to v0.10.0 covering all Phase 7 ecosystem tracks (2530 lines, 17 subsystem sections)
- **WorkflowRegistry** – YAML workflow loading from `workflows/` directory with intent/complexity-based matching
- **Session types** – `SessionSnapshot` expanded to 7 fields (snapshot_id, current_node_id, ExecutionState, execution_context_id, trace_id, checkpoint_timestamp_ms)

### Removed
- `PipelineStage` variants reordered; `Scheduling` and `Execution` remain separate discriminant values

## [0.9.0] – 2026-07-25

### Added
- **Fusion Strategy Engine** – `FusionStrategy` at `src/strategies/fusion.rs` with `ModelAvailability`/`ModelCapability` hints and `with_model_hints()` builder; produces heterogeneous `N × LLMGenerate → LLMJudge` PrimitiveGraph
- **Provenance Schema** – `primitive_graph_hash: u64` on `ExecutionGraph` as deterministic provenance fingerprint via canonical serde_json serialization
- **WASM Strategy Bridge** – 5-export FFI contract (`memory`, `fusion_strategy_name`, `fusion_strategy_descriptor`, `fusion_strategy_lower`, `alloc`) in `src/plugin/wasm.rs`
- **Per-Strategy Metrics** – `fusionrouter_strategy_latency_seconds` / `_errors_total` histograms + `graph_hash_count` counter
- **Optimization Passes** – `DeadNodeEliminationPass` + `FanOutConsolidationPass` under `OptimizationPipeline` with rollback safety; 7 legality rules per ADR-020
- **PrimitiveGraph IR** – canonical lowered IR with `compute_hash()`, `to_execution_graph()`, `to_mermaid()`, `to_dot()`; versioned at `PRIMITIVE_GRAPH_VERSION = 1`
- **Strategy SDK** – `Strategy` trait with `descriptor()` + `lower()` returning `PrimitiveGraph` fragments; `StrategyIR` enum for strategy-neutral lowering
- **Golden Tests** – 16 golden tests for optimization passes across DNE + FanOut consolidation
- **Strategy Benchmarks** – 10 scenarios across 7 strategy types (512ns Single → 18µs Consensus/5) in `benches/strategy_lowering.rs`
- **Structured Tracing** – Pipeline events (`pipeline.started`, `pipeline.completed`, `pipeline.failed`, `strategy.lowered`, `strategy.executed`) with `request_id`, `strategy`, `latency_ms`, `success` fields
- **Multi-Model Consensus Review Script** – `scripts/consensus_review.ps1`: reusable template for parallel architect models + judge synthesis; 7 configurable parameters; architects run via parallel PowerShell jobs; supports any provider prefix
- **Capability-Based Provider Selection** – decouples routing from hardcoded model prefixes
  - `ModelRequirements` struct with 8 fields (`min_context_tokens`, `min_coding_score`, `min_reasoning_score`, `requires_tools`, `requires_streaming`, `requires_vision`, `max_cost_per_1k_tokens`, `preferred_provider`) and `matches(&self, caps, pricing) -> bool` method
  - `RequirementsExtractor.build_model_requirements()` sets coding/reasoning thresholds by intent, enables `requires_tools` when tools present, sets `min_context_tokens` for large payloads
  - `ProviderRegistry` with `register_target_with_capabilities()`, `select_targets(reqs)` that filters by capability match, sorts by cost ascending, excludes open circuits
  - `ModelResolutionPass.select_model()` picks code/architecture/fast model based on requirements
  - `ProviderRouter.resolve_target()` falls back to `registry.select_targets()` when no prefix match
- **Continuous Feedback Calibration** – closed-loop capability adjustments from telemetry
  - `ModelPerformanceStats` struct and `get_model_stats(window_hours)` on `EvidenceRepository` trait
  - SQL aggregation with `HAVING COUNT(*) >= 30` cold-start guardrail
  - `FeedbackCalibrator` engine: health penalty factor on `success_rate < 0.95`, EMA smoothing (α=0.2), `min_score_floor = 0.1`
  - `spawn_calibration_loop()` background Tokio task at configurable interval
  - `get_capabilities()` / `update_capabilities()` on `ProviderRegistry` with atomic version bump
  - 3 unit tests: cold-start skip, penalty, recovery
- **Circuit Breaker** – `CircuitBreaker` (Closed/Open/Half-Open state machine) and `CircuitBreakingProvider` wrapper with configurable thresholds, cooldown, and success recovery; integrated into `ProviderRouter` for automatic fallthrough on failure
- **ModelCatalog** – typed struct mapping roles (code, debug, architecture, general, creative, analysis, fast, cheap) to model names with sensible defaults
- **Scheduler Concurrency** – `max_concurrent_nodes` config driving `buffer_unordered` in `DefaultScheduler` for bounded parallel execution; `Backoff` full-jitter retry on node failures
- **NodeExecutionResult.output** – per-node output capture propagated to `ExecutionResult.outputs` for downstream access
- **OTel Tracing** – `telemetry::tracing` module with OpenTelemetry gRPC export (feature-gated by `otel`); `dev-console` feature for tokio-console subscriber
- **Resource Management**
  - `BudgetEnvelope` – atomic cost/token/iteration tracking with `record_and_check()` enforcement
  - `ResourceGuard` – RAII-style commit/rollback for cross-request quota safety
  - `ResourceManager` trait with `reserve()`, `commit()`, `rollback()` for concurrency-safe budgeting
- **Pipeline Server Module** – `PipelineContext` with typed per-request state (request, context, requirements, IR, graph, resource guard, execution result, response, budget envelope); supports cancellation via `CancellationToken`
- **CI Hardening** – GitHub Actions workflows for test, clippy, cargo-audit, cargo-deny; `deny.toml` for license + vulnerability checking
- **Benchmarks** – Criterion benchmarks for compilation throughput and cache performance (`benches/compilation.rs`, `benches/cache.rs`)

### Changed
- **ZenProvider Transport Timeout** – increased from 30s → 300s (`src/providers/zen.rs:7`) to accommodate long-running prompts on OpenCodeZen API (observed >100s for medium-length judge prompts)
- **HNSW Vector Cache** – semantic cache upgraded from brute-force `Vec<CacheEntry>` linear scan to HNSW (`usearch`) with `HashMap<u64, CacheEntry>` for O(log n) nearest-neighbor search; configurable dimensions, connectivity, and expansion params
- **Transport Layer** – unified `Transport` trait with typed `TransportRequest`/`TransportResponse`/`TransportEvent`/`TransportError` structs; `HttpTransport` with retry + full-jitter exponential backoff; `StdioTransport` and `WebSocketTransport` refactored to new signatures
- **ProviderRouter** – wrapped providers in `CircuitBreakingProvider`; changed from single-provider resolve to multi-provider fallthrough with circuit-breaker skip
- **SqliteEvidenceRepository** – `record()` and `snapshot()` migrated from synchronous `Mutex` to `tokio::task::spawn_blocking` with `Arc<Mutex<Connection>>` for non-blocking async operation
- **Scheduler** – `DefaultScheduler` accepts `max_concurrent` param; execution uses `buffer_unordered` for bounded parallelism; retries use `Backoff` full-jitter backoff instead of fixed delay; `NodeExecutionResult` gains `output` field propagated to `ExecutionResult`
- **Observability** – `tracing::instrument` on 17 key methods across scheduler, executor, transport; optional OTel (`otel` feature) and tokio-console (`dev-console` feature) subscribers; subscriber initialized at startup
- **Budget Pass** – graph lowering eliminated; budget enforcement moved to runtime via `BudgetEnvelope`
- **WorkQueue** – loop back-edges excluded from initial `total_incoming` counts for clean loop header initialization
- **Zero Rust Warnings** – all dead_code and unused imports removed or annotated
- **Compiler pipeline** – `DefaultCompiler::compile()` runs passes sequentially with snapshot-and-rollback transactional semantics

### Fixed
- ProviderRegistry replaced `tokio::sync::watch` with `Arc<AtomicU64>` to resolve Sync constraint preventing `Arc<ProviderRegistry>: Send`

## [0.8.0] – 2026-07-18

### Added
- **Intent-Oriented Planner** (ADR-016) – public API expresses *intent* via `execution` field; planner compiles to internal graph
  - `ExecutionIntent` enum: `Quality`, `Speed`, `Balanced`, `Exhaustive`, `Constrained`
  - `IntentPlanner` maps each intent to a multi-node `WorkflowIR` (Quality=5, Speed=1, Balanced=3, Exhaustive=6, Constrained=budget-aware)
  - `OutputPreferences` with `include_report` for optional `ExecutionReport` in responses
  - `ExecutionReport` struct: graph summary, per-model costs, timing, model breakdown, decisions
  - `ChatCompletionRequest` extended with `execution` and `output` fields
- **Judge/Reflect system prompts** – `DefaultExecutor` injects judge/reflect system prompts for `LLMJudge` and Reflection strategy nodes
- **31 new tests** covering intent types, planner variants, JSON serialization, handler integration

### Changed
- `AppState::new()` now uses `IntentPlanner` instead of `WorkflowPlanner`
- Handler pipeline passes `execution` and `output` from request to `Requirements`

## [0.7.1] – 2026-07-18

### Added
- **OpenCode integration** – example config (`examples/opencode/opencode.json`), setup scripts (`scripts/setup-opencode.sh`, `scripts/setup-opencode.ps1`), QUICKSTART.md section
- **Rate limiting opt-in** – `enabled: bool` field on `rate_limiting` config (default `false`); middleware is now conditional

### Changed
- **Strict request validation** – `ChatCompletionRequest` now uses `serde(deny_unknown_fields)`; unknown fields like `strategy` return 422 instead of being silently ignored

## [0.7.0] – 2026-07-18

### Added
- **API Key Authentication** – config-driven middleware checking `x-api-key` header; whitelisted paths: `/health`, `/ready`, `/metrics`; opt‑in via `auth.enabled`
- **CORS Middleware** – config-driven `allowed_origins`, `allowed_methods`, `allowed_headers` with wildcard support
- **Token-Bucket Rate Limiting** – per-client (identified by `x-api-key` or `x-forwarded-for`) with configurable RPM, burst, cleanup; opt‑in via `rate_limiting.enabled`
- **Health Check Endpoints** – `/health` (always ok), `/ready` (dependency checks)
- **Graceful Shutdown** – Ctrl+C / SIGTERM handler with `shutdown_timeout_secs` support
- **Structured JSON Logging** – config-driven format selection (`text` or `json`)
- **Request ID Tracing** – UUID generation, passthrough of `x-request-id` header, response header injection
- **Configuration Validation** – 11 checks on startup (port, timeout, auth keys, rate limits, log format)

### Changed
- Server address now reads from config (`server.host` / `server.port`)
- Tracing subscriber initialized after config load to respect `logging.format` / `logging.level`

## [0.6.0] – 2026-07-17

### Added
- **HTTPRequestTool** – GET/POST/PUT/DELETE with configurable headers and 30s timeout
- **ShellCommandTool** – allowed-list safety guard with timeout
- **ToolRegistry utilities** – `len()`, `contains()`, `unregister()`
- **Plugin tool support** – `[tool]` section in manifests, `register_tool()`
- **Tool config** – `tools` section in `config/default.yaml`
- **Tool dispatch in executor** – JSON tool invocation: `{"tool": "name", "args": {...}}`
- **Tool loading in AppState** – Calculator, ShellCommand, HTTPRequest, FileRead pre‑registered
- **Integration tests** – ReAct + tool registry golden tests
- **Safety guards** – allow-list for shell, path canonicalization for file read

### Fixed
- ReAct strategy now correctly passes `available_tools` to generator nodes

## [0.5.0] – 2026-07-17

### Added
- **Dynamic Workflow Generation** (`DynamicPlanner`) – LLM generates `WorkflowIR` from requirements via prompt, validated through existing compiler passes
  - ADR-015 documents the approach with safety guards
  - `PlannerMode` enum: `Static`, `Dynamic`, `Hybrid`
  - `DynamicPlannerConfig`: `max_generated_nodes` (20), `generation_timeout` (10s), `max_iterations` (10)
  - Falls back to `SimplePlanner` on validation failure
  - 4 unit tests for JSON IR parsing and safety limits
- **Tool Registry** – pluggable tool system for ReAct and other strategies
  - `Tool` trait with `name`, `description`, `schema`, `execute`
  - `ToolRegistry` with `register`, `get`, `list`
  - Built-in tools: `CalculatorTool` (arithmetic), `SearchTool` (mocked), `FileReadTool` (with path traversal protection)
  - `ReActStrategy` now accepts optional `Arc<ToolRegistry>`
- **Semantic Caching** – embedding-based response cache
  - `Embedder` trait with `MockEmbedder` (384-dim deterministic vectors)
  - `SemanticCache` with configurable similarity threshold and max entries
  - LRU eviction when cache exceeds max entries
  - Integrated into `DefaultExecutor`: cache check before provider call, store after
- **NodeExecutionResult** – structured per-node execution metadata
  - `Usage` field tracking `prompt_tokens`, `completion_tokens`, `total_tokens`
  - Token/cost accumulation in `DefaultScheduler` with per-token cost rates
  - Non-zero metrics propagated to `ExecutionResult`
- **Prometheus Metrics** endpoint at `/metrics`
  - Counters: `fusionrouter_requests_total`, `errors_total`, `tokens_total`
  - Histograms: `request_duration_seconds`, `provider_latency_seconds`
- **Audit Log** – structured in-memory audit trail with JSONL export
- **WebSocket & Stdio Transports** – `Transport` trait with two new implementations
- **Disconnected subgraph cycle detection** – golden test for `detect_cycle_disconnected_subgraph`
- `IRNodeKind` gains `PartialEq` derive

### Changed
- `Executor::execute_node` returns `NodeExecutionResult` instead of `Result<NodeState, anyhow::Error>`
- `Scheduler` trait: `create_instance` removed, `schedule` method now creates the instance
- Plugin golden test cleaned up (removed unused `HashMap` import)
- Version bumped to `0.5.0`

### Fixed
- Token/cost accumulation no longer stubbed at zero in `DefaultScheduler`
- `FileReadTool` uses canonical path resolution for proper path traversal protection

## [0.4.0] – 2026-07-17

### Added
- **Chain Strategy** – sequential pipeline of sub-strategies; each stage feeds into the next via `ExecutionEdge`
- **ReAct Strategy** – reasoning loop with `Loop` control node and configurable `max_iterations`; mimics the ReAct (Reasoning + Acting) pattern
- **Debate Strategy** – parallel debaters feeding into a judge strategy for adversarial refinement
- 5 golden tests verifying subgraph structure for each new strategy

### Changed
- `StrategyKind::ReAct` added to the strategy enum

## [0.3.0] – 2026-07-17

### Added
- **Workflow Registry** – named workflow definitions with YAML DSL
  - `WorkflowDefinition` struct with name, description, capability filters, node/edge templates
  - `WorkflowRegistry` with register, get, list, load_dir, select methods
  - YAML-based workflow definitions auto-loaded from `workflows/` directory
  - Example workflows: `code-review`, `chat`, `deep-research`
- **WorkflowPlanner** – DAG planner that matches `Requirements` to registered workflows
  - Selects workflow definition matching intent and complexity
  - Falls back to `SimplePlanner` when no workflow matches
  - `instantiate()` converts definition to `WorkflowIR` guided by `Requirements`
- **Requirements Struct Migration** – typed fields replacing string maps
  - `intent` renamed to `intent_classification`
  - `Complexity` renamed to `ComplexityLevel`
  - Added `has_files`, `context_window`, `original_text` fields
  - Removed `soft_scores` and `hard_constraints` maps

### Changed
- Planner pipeline now uses `WorkflowPlanner` by default with `SimplePlanner` fallback

## [0.2.1] – 2026-07-17

### Added
- Structured `CompilerError` with typed `ValidationError { pass, node_id, message }` and `PassError { pass, message }` variants
- 3-color DFS cycle detection in `ControlFlowValidationPass` (replaces ad-hoc DFS)
- `total_tokens` and `total_cost` fields on `ExecutionGraph`

### Fixed
- Cycle detection now follows standard white/grey/black coloring
- Error messages include pass name and failing node ID

## [0.2.0] – 2026-07-17

### Added
- **Plugin System** – dynamic loading for providers, strategies, and compiler passes
  - `PluginRegistry` with discovery from `plugins/` directory
  - TOML-based manifests for plugin metadata
  - `libloading`-based dynamic loading (C ABI)
  - Sample plugin (`example-provider`) demonstrating the ABI
- Plugin registration for providers, strategies, and compiler passes
- 5 golden tests for plugin functionality

### Changed
- Workspace configuration for plugin crates (`plugins/` directory)

## [0.1.0] – 2026-07-17

### Added
- Full DAG support (conditional, loop, split, join, barrier nodes)
- Provider/Model/Transport split with HTTP transport (Zen, OpenRouter, Ollama)
- Compiler pipeline with 4 passes (validation, control-flow, model resolution, budget)
- Resource manager with atomic quota tracking (cost + tokens)
- Evidence repository (SQLite-backed) for telemetry
- Strategies: Single, Consensus, Reflection
- Streaming support (SSE) with `text/event-stream`
- Full pipeline integration: context assembler → requirements extractor → planner → compiler → scheduler → executor → telemetry
- Configuration loading from YAML (`config/default.yaml`)
- 30 tests across unit, golden, integration, and load test suites
- Comprehensive documentation: architecture, runtime, workflow IR, execution graph, ADRs

### Fixed
- BudgetOptimisationPass now correctly integrates with ResourceManager
- Context trimming preserves system messages, drops oldest history
- Cross-request quota enforcement with atomic reservation/release
- Conditional edge activation (only matching branch runs)
- Scheduler handles loop-back edges with iteration limits

### Changed
- Replaced monolithic Provider trait with Model/Transport/Provider composition
- all dead_code warnings suppressed as expected for evolving architecture
- All 6 ADRs updated to reflect final design decisions
