# Changelog

## [0.1.0] – 2025-07-17

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
