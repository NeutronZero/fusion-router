# ADR-017: Runtime Event Stream ABI & Observability Substrate

* **Status:** Approved
* **Date:** 2026-07-27
* **Subsystem:** Runtime Intelligence / Events

---

## Context

Prior to v0.11, FusionRouter was primarily an execution-centric DAG engine. As the platform expands toward OpenTelemetry tracing, timeline visualizers, node-level checkpointing, append-only event persistence, and future Execution Memory (RationaleVault), instrumenting the execution loop independently for each subsystem increases coupling and performance overhead.

---

## Decision

We establish the **Runtime Event Stream** as the official **Runtime Observability ABI** between the execution engine and all downstream observability, storage, and recovery subsystems.

### Core Architectural Invariants

1. **Event Sourcing as Primary Substrate:** All execution state transitions, compilation results, provider calls, tool invocations, and resource allocations are emitted as append-only, strongly-typed `ExecutionEvent` variants.
2. **Immutable Events After Publication:** Execution events are strictly immutable once published. No projection, store, or bus may edit or rewrite an event envelope. Corrective actions or retries are represented by *new* events.
3. **Monotonic Sequence & Correlation Identity:** Every event envelope carries `schema_version` (`fusion.router.event.v1`), `event_id`, `workflow_id`, `execution_id`, `correlation_id` (for logical sub-operations), `sequence_number` (1-based monotonic index per execution), `timestamp`, and optional `parent_event_id`.
4. **Interface-Driven Bus Abstraction:** Runtime components publish events via an abstract `EventBus` trait (`publish`, `subscribe`). `BroadcastEventBus` (backed by `tokio::sync::broadcast`) serves as the initial implementation.
5. **Decoupled Projection Framework:** Observability, tracing, checkpointing, and storage consume events via `EventProjection` implementations. Projections run asynchronously and isolated on background tasks.
6. **Projection Isolation & Non-Interference:** Projection panics, delays, or errors MUST NOT block, slow down, or crash the core runtime execution loops.

---

## Consequence

- **Pros:** Zero-coupling runtime extensions. Observability, timeline rendering, checkpointing, and memory are additive projections.
- **Cons:** Serialization overhead for high-frequency events, mitigated by async zero-copy channels (`tokio::sync::broadcast`).
