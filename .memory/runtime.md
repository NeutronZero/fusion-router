# FusionRouter Runtime

## Overview

The runtime manages execution of the compiled `ExecutionGraph` as a DAG, handling scheduling, node dispatch, resource management, session continuity, and event emission.

**Design doc:** `docs/architecture/runtime.md`

## DAG Execution Model

The runtime executes nodes from the `ExecutionGraph` in topological order via a `WorkQueue`.

### Node Kinds (12 types)

| Node Kind | Behavior |
|-----------|----------|
| `LLMRequest` | Single LLM call via Provider |
| `Strategy` | Multi-step reasoning strategy (expands to subgraph) |
| `ToolCall` | Tool invocation |
| `Connector` | Connector operation (GitHub, Browser, etc.) |
| `Conditional` | Branch: evaluates condition → activates one outgoing edge |
| `Loop` | Iteration: boolean check → re-enqueue or exit |
| `Split` | Fan-out to parallel execution paths |
| `Join` | Barrier: synchronizes parallel paths before proceeding |
| `Barrier` | Synchronization point |
| `Transform` | Data transformation |
| `Gate` | Policy gate check |
| `Subgraph` | Nested subgraph invocation |

### Node State Machine

```
Pending → Running → Succeeded
                  → Failed (retryable → Pending)
                  → Failed (terminal)
```

### Scheduling Algorithm

1. Query `WorkQueue` for ready nodes (all dependencies satisfied)
2. Mark nodes as `Running`
3. Execute concurrently via `tokio::join_all`
4. Process completions:
   - Normal: mark succeeded, enqueue dependents
   - Conditional: evaluate condition → activate matching edge
   - Loop: check boolean → re-enqueue body or activate exit edge
   - Split: activate all outgoing edges
   - Join: countdown barrier, activate when all arrived
   - Fail: apply retry policy or mark terminal

## Scheduler

**Location:** `src/scheduler/`

| Component | File | Purpose |
|-----------|------|---------|
| `Scheduler` trait | `src/scheduler/mod.rs` | Core scheduler interface |
| `DefaultScheduler` | `src/scheduler/default.rs` | Local DAG scheduler |
| `WorkQueue` | `src/scheduler/work_queue.rs` | Topological work queue |
| `DistributedScheduler` | `src/scheduler/distributed.rs` | Remote worker pool scheduling |
| `ConnectorResolver` | `src/scheduler/connector_resolver.rs` | Late-binding of connectors |
| `ConnectorSubscriber` | `src/scheduler/connector_subscriber.rs` | Connector event subscription |
| `ConnectorHealth` | `src/scheduler/connector_health.rs` | Connector health monitoring |

## Session Lifecycle

**Location:** `src/session/`, `src/lifecycle/`

| Concept | Type | Description |
|---------|------|-------------|
| Execution Session | `ExecutionSession` | Identity container for a single execution (ADR-026) |
| Session Snapshot | `SessionSnapshot` | Point-in-time capture for replay (ADR-030) |
| Checkpoint Engine | `CheckpointEngine` | Creates snapshots at configurable intervals |
| Replay Engine | `ReplayEngine` | 3 replay modes: Deterministic, Inspection, Simulation |
| Session Store | `SessionStore` trait | Memory, SQLite backends |

## Resource Management

**Location:** `src/resource/`

| Component | Purpose |
|-----------|---------|
| `ResourceManager` | Central resource tracking |
| `ResourceGuard` | RAII guard for resource cleanup |
| `BudgetEnvelope` | Token/time budget enforcement |
| `StreamMeter` | Streaming token counting |
| `CancellingStream` | Safe stream cancellation |

### Streaming Cost Accounting

- **Quota is enforced, not just reserved**: streaming requests are
  pre-flighted against the `BudgetEnvelope` via
  `ResourceManager::try_reserve` before the graph runs; an over-quota stream
  request fails fast with `QuotaExceeded` instead of consuming the scarce
  enqueue slot and failing mid-stream.
- **Usage is recorded on stream finish, never discarded**: streams run
  through `metered_stream_with_finish` (`src/resource/cancelling_stream.rs`),
  whose `StreamFinishHook` debits the real token usage exactly once — on
  completion, error, cancellation, or drop — via
  `ResourceManager::record_usage`. The `ResourceGuard` is held for the whole
  stream and captures a `tokio::runtime::Handle` at construction so the
  debit survives cross-task drop.
- `StreamMetrics::record_report` / `record_error` capture the final
  meter report and per-stream failure telemetry.

## Trigger Framework

**Location:** `src/trigger/`

| Component | Purpose |
|-----------|---------|
| `ExecutionRequest` | Canonical request type (ADR-031) |
| `WebhookHandler` | Incoming webhook triggers |
| `CronHandler` | Scheduled execution triggers |
| `EventBusHandler` | Internal event-driven triggers |
| `TriggerTrace` | Provenance chain for triggered executions |

## Event System

**Location:** `src/events/`
**ADR:** ADR-017 (Runtime Event Stream ABI)

| Component | Purpose |
|-----------|---------|
| `ExecutionEventEnvelope` | Immutable event envelope (monotonic sequence) |
| `EventBus` trait | Core event bus interface |
| `BroadcastEventBus` | In-memory broadcast implementation |
| `ProjectionDispatcher` | Decouples event production from consumption |
| Consumers: Timeline, Storage, OTel, Checkpoint | Projection targets |

## Key Invariants

- `ExecutionGraph` is never mutated by runtime
- Scheduler owns output selection
- Executor dispatches via `CapabilityExecutor`
- Evidence is written post-execution, never during
- Resource cleanup guaranteed via RAII
- Telemetry never blocks request processing
