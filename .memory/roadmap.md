# FusionRouter Roadmap

## Current: v0.12.0 — Capability Platform

### Delivered
- Capability Registry immutable architecture (ADR-021)
- Capability Resolution with dependency graph (ADR-023)
- Capability SDK with macros and builder API
- Policy compilation pass (ADR-024)
- Connector abstraction late-binding (ADR-025)
- Execution session stores (Memory, SQLite) (ADR-026)
- Capability contract evolution with SemVer (ADR-028)
- Capability Binary Interface (`.fusionpkg`)
- Capability Host Interface (WASM host services)
- Capability Runtime (`SandboxRuntime`, `WasmtimeSandboxRuntime`, `RuntimeModuleCache`)
- Package Platform (`.fusionpkg` verify/load/registry)
- Developer Platform (`fusion new/build/test/publish/dev`)
- Operations Platform (`/v1/operations/*` REST API)
- Certification and ecosystem tooling

## v0.9.0 — Foundation

- Core pipeline: Context Assembly → Requirements → Planner → Compiler → Scheduler → Executor
- Provider abstraction (OpenRouter, Ollama, Zen)
- 3 strategy types (Single, Consensus, Reflection)
- SQLite Evidence Repository
- Resource Manager with RAII guards
- Config management (YAML + env vars)
- Basic CLI

## v0.10.0 — Production Hardening

- 7 strategy types (added Debate, ReAct, Chain, Fusion)
- Distributed Scheduler with RemoteWorkerPool
- Session continuity: ExecutionSession, snapshots, replay
- Semantic cache (USearch HNSW, feature-gated)
- Trigger framework (Webhook, Cron, EventBus)
- 6 connectors (GitHub, Browser, MCP, Filesystem, HTTP, Shell)
- Developer tools (GraphVisualizer, TraceInspector)
- Feedback calibration (EMA)
- Prometheus metrics

## v0.12.0 — Capability Platform

Released 2026-07-31. See "Current" section above for the delivered feature list. Pre-release plan history (2026-07-30): the five subsystem plans under `docs/superpowers/plans/2026-07-30-v0.12-*.md`.

## ADR Status

| Status | Meaning |
|--------|---------|
| Accepted | Design decision has been made and implemented |
| Accepted (Frozen) | Design is final, no further changes expected |
| Proposed | Design is under consideration, not yet implemented |
| Approved (in `docs/adrs/`) | Design approved, separate tracking |
