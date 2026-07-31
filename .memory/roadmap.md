# FusionRouter Roadmap

| Version   | Theme                           | Status       |
|-----------|---------------------------------|--------------|
| v0.12.x   | Capability Platform             | Complete     |
| v0.13.0   | Architecture Freeze             | Complete     |
| **v0.13.1** | **Compiler Core**              | **Next**     |
| v0.13.2   | Intelligent Runtime             | Planned      |
| v0.13.3   | Adaptive Optimization           | Planned      |
| v0.14.0   | Distributed Execution           | Future       |

The **v0.13.x** series is the implementation series for the frozen v0.13.0 architecture.

## Current: v0.13.0 (Architecture Freeze)

- Six core abstractions frozen: NormalizedIntent, WorkflowIR, ExecutionAbi, ExecutionTarget, ERI, CapabilityRegistry (ADR-033)
- Architecture specification published: `docs/specifications/architecture-v0.13.md`
- Execution ABI defined separately from compiler-internal PrimitiveGraph (ADR-032)
- Capability traits added to the capability contract (`CapabilityTrait`)
- Reconciliation design: `docs/superpowers/specs/2026-07-30-v0.13-reconciliation-design.md`

## Next: v0.13.1 — Compiler Core

Compiler pipeline: NormalizedIntent → WorkflowIR → Compiler (Semantic / Optimizer / Correctness / ABI Generator) → Execution ABI v1.

Implementation priority:

1. Workflow IR data model and builder
2. Execution ABI v1 schema
3. Compiler pass pipeline and pass registration
4. ABI generation pass
5. Capability Registry execution and lookup
6. Runtime contract implementation consuming the ABI (ERI integration)
7. End-to-end compile path: NormalizedIntent → WorkflowIR → ExecutionAbi → Runtime

Deliverable: first complete compile-and-execute path (optimizations may remain stubbed).

## Planned: v0.13.2 — Intelligent Runtime

- ERI-based runtime contract implementations, ABI consumption, execution state model (9 states)

## Planned: v0.13.3 — Adaptive Optimization

- Adaptive loop, optimizer stages, cost model, optimization levels (O0–O3)

## Future: v0.14.0 — Distributed Execution

- Next major architectural capability; requires ADR-driven changes before implementation

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
