# FusionRouter Execution Model

## Overview

The execution model governs how compiled `ExecutionGraph` nodes are dispatched, how sessions are managed, and how replay/correctness guarantees are enforced.

**Location:** `src/executor/`, `src/session/`, `src/lifecycle/`

## Executor (`src/executor/`)

| Component | File | Purpose |
|-----------|------|---------|
| `Executor` trait | `src/executor/mod.rs` | Core executor interface |
| `DefaultExecutor` | `src/executor/mod.rs` | Standard executor implementation |
| `CapabilityExecutor` | `src/executor/capability_executor.rs` | Unified capability execution dispatch |

The executor dispatches scheduled nodes to the appropriate handler:

- **LLM nodes** → `Provider` trait (model call via transport)
- **Strategy nodes** → `Strategy` trait (multi-step reasoning subgraph)
- **Tool nodes** → `Tool` trait (built-in tools via `ToolRegistry`)
- **Connector nodes** → `Connector` trait (external service connectors)
- **Capability nodes** → `CapabilityExecutor` (unified capability dispatch)

## Execution State Machine (ADR-029)

```
Pending ──→ Resolved ──→ Scheduled ──→ Running ──→ Succeeded
                                              ──→ Failed (retryable → Resolved)
                                              ──→ Failed (terminal)
                                              ──→ Cancelled
```

### Event Emission

Each state transition emits an `ExecutionEventEnvelope` with monotonic sequencing (ADR-017). Events are immutable after emission.

### Guarantees

| Property | Mechanism |
|----------|-----------|
| Cancellation | Token-based cancellation propagation |
| Timeouts | Per-node timeout enforcement, `BudgetEnvelope` |
| Idempotency | Deterministic UUIDs on all graph nodes |
| Retry | Configurable retry policy with exponential backoff |
| Isolation | Session-scoped execution context |

## Execution Session (ADR-026)

| Concept | Type | Description |
|---------|------|-------------|
| Identity | `ExecutionSession` | Container for a single execution run |
| Snapshot | `SessionSnapshot` | Point-in-time capture of execution state |
| Store | `SessionStore` trait | `MemorySessionStore`, `SqliteSessionStore` |

## Session Continuity (ADR-030)

### Three Replay Modes

| Mode | Behavior |
|------|----------|
| `Deterministic` | Exact replay: same inputs, same order, same results |
| `Inspection` | Read-only exploration of a past execution |
| `Simulation` | What-if analysis with modified parameters |

### Checkpoint Engine (`src/session/checkpoint.rs`)

- Configurable checkpoint intervals (node count, time, manual)
- Captures full `SessionSnapshot` at each checkpoint
- Enables replay from any checkpoint boundary

### Compatibility Validation

On session resume, validates that the runtime version, capability contracts, and provider configurations are compatible with the snapshot's recorded state.

## Trigger Framework (ADR-031)

**Location:** `src/trigger/`

| Type | Description |
|------|-------------|
| `ExecutionRequest` | Canonical request type — single-pipeline invariant |
| `TriggerTrace` | Provenance chain: trigger source → policy → execution |
| Webhook | Incoming HTTP-triggered executions |
| Cron | Scheduled time-based executions |
| EventBus | Internal event-driven executions |

### Invariants

- All requests follow the single-pipeline: trigger → context assembly → planner → compiler → scheduler → executor
- Request payload is immutable after reception
- Deduplication via `TriggerId` / `ProvenanceId`
- Unified provenance chain: `TriggerTrace → PolicyTrace → ExecutionTrace`

## Lifecycle Manager (`src/lifecycle/manager.rs`)

Orchestrates the full session lifecycle:
1. Session creation and initialization
2. Checkpoint placement
3. Session suspension and resumption
4. Session teardown and cleanup

## Related ADRs

- ADR-016: Intent-oriented execution (clients express intent, not mechanics)
- ADR-017: Execution Runtime ABI (scheduler loop, output selection)
- ADR-026: Execution Session (session/store decoupling)
- ADR-029: Execution Semantics (formal state machine, frozen)
- ADR-030: Session Replay Semantics (3 replay modes, frozen)
- ADR-031: Trigger Request Semantics (canonical request, frozen)
