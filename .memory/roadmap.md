# FusionRouter Roadmap

## Current: v0.13.0 (Architecture Freeze)

- Six core abstractions frozen: NormalizedIntent, WorkflowIR, ExecutionAbi, ExecutionTarget, ERI, CapabilityRegistry (ADR-033)
- Architecture specification published: `docs/specifications/architecture-v0.13.md`
- Execution ABI defined separately from compiler-internal PrimitiveGraph (ADR-032)
- Capability traits added to the capability contract (`CapabilityTrait`)
- Reconciliation design: `docs/superpowers/specs/2026-07-30-v0.13-reconciliation-design.md`

## Next: v0.14.0 — Compiler Core

- Workflow IR implementation
- Execution ABI v1
- Compiler pass framework
- Capability registry
- Local runtime

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
