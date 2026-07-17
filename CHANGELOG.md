# Changelog

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
