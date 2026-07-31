# Memory Changelog

## v1.0 (2026-07-30)

Initial architecture knowledge base for FusionRouter.

### Added
- **overview.md** — Project vision, pipeline overview, module catalog, coding rules, core invariants
- **architecture.md** — System architecture, data models, compiler passes, strategy types, 16 invariants, feature flags
- **compiler.md** — Compiler pipeline, IR representations, optimization framework, ADR references
- **runtime.md** — DAG execution model, node kinds, state machine, scheduling algorithm, session lifecycle, events
- **planner.md** — Planner implementations, capability resolver, workflow registry, WorkflowIR node types
- **capability-system.md** — Capability registry, resolver, graph, permissions, SDK, plugin API, package format
- **execution.md** — Executor dispatch, formal state machine, replay modes, checkpoint engine, trigger framework
- **providers.md** — Provider/Model/Transport abstraction, circuit breaker, transport implementations, model adapters
- **scheduler.md** — Scheduler components, DAG execution, control flow, distributed scheduling
- **policies.md** — Policy AST/IR, compilation pass, release governance, 8 release gates, attestation
- **telemetry.md** — Event system, projections, evidence repository, metrics, tracing, audit, DevEx tools
- **plugin-system.md** — Extension points, WASM runtime, host interface, plugin types, ABI versioning
- **roadmap.md** — Version roadmap (v0.9, v0.10, v0.11, v0.12+), ADR status legend
- **glossary.md** — 40+ terms with definitions
- **adrs.md** — All 34 ADRs indexed with status and summaries
- **module-index.md** — 80+ types/components with file locations

### Added (v1.0 follow-up)
- **architecture-map.md** — One-page ASCII pipeline map with subsystem references and layer rules
- **editing-guide.md** — Operational edit knowledge: component boundaries, common task templates, safety rules
- **scripts/check-memory.py** — Automated validation: file paths, ADR references, cross-references, module-index staleness
- **VERSION** — Schema version tracking for divergence detection
- **CHANGELOG.md** — This file
