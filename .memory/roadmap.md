# FusionRouter Roadmap

| Version   | Theme                           | Status       |
|-----------|---------------------------------|--------------|
| v0.12.x   | Capability Platform             | Complete     |
| v0.13.0   | Architecture Freeze             | Complete     |
| v0.14.1   | Engine Unification              | Complete     |
| v0.14.2   | Studio BFF Vertical Integration | Complete     |
| v0.14.3   | Distributed Execution Primitives| Complete     |
| v0.14.4   | Systems Audit & Hardening       | Complete     |
| v0.14.5   | Realistic Validation under Load | Next         |

The **v0.13.x** series is the implementation series for the frozen v0.13.0 architecture.

## Current: v0.13.0 (Complete)

Architecture frozen.

Key deliverables:

- Architecture specification
- ADR-032 / ADR-033
- Six core abstractions frozen
- Provider-free compiler contracts

## Next: v0.13.1 — Compiler Core

Compiler pipeline: NormalizedIntent → WorkflowIR → Compiler (Semantic / Optimizer / Correctness / ABI Generator) → Execution ABI v1.

Implementation priority:

1. - [x] Workflow IR data model and builder — Implemented: Workflow IR data model + builder (crates/fusion-ir) — Tasks 1-8 of the fusion-ir plan.
2. - [x] Execution ABI v1 schema — Implemented with `abi::from_graph` (ABI generator) + `abi::to_graph` (runtime binding).
3. - [x] Compiler pass pipeline and pass registration — Implemented as `build_compiler` (ADR-034, live v0.12 path).
4. - [x] ABI generation pass — `abi::from_graph::abi_from_graph` bridges the compiled `ExecutionGraph` to the contract.
5. Capability Registry execution and lookup
6. - [x] Runtime contract implementation consuming the ABI (ERI integration) — `eri::local_runtime::LocalEri` executes ABIs on the live scheduler/executor.
7. - [x] End-to-end compile path: NormalizedIntent → WorkflowIR → ExecutionAbi → Runtime — verified by `tests/contract_wiring.rs` (intent → IR adapter → compile → ABI → ERI execution).

Deliverable: first complete compile-and-execute path (optimizations may remain stubbed).

Success Criteria:

- WorkflowIR implemented
- Execution ABI v1 implemented
- Compiler pipeline operational
- Capability Registry executable
- Local Runtime executes Execution ABI
- End-to-end compile path passes integration tests

## Planned: v0.13.2 — Intelligent Runtime

- Execution scheduler
- Provider resolution
- Retry engine
- Streaming
- Execution state machine
- Circuit breaker
- Rate limiter
- Runtime telemetry

## Planned: v0.13.3 — Adaptive Optimization

- Cost model
- Compiler optimizer stages
- Optimization levels
- Telemetry feedback
- Adaptive heuristics
- Cache optimization

## Future: v0.14.0 — Distributed Execution

- Execution Coordinator
- Worker Runtime
- Remote ABI execution
- Distributed scheduling
- Checkpointing

Requires ADR-driven changes before implementation.

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
