# Changelog

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
