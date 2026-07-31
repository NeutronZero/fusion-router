# FusionRouter Scheduler

## Overview

The scheduler coordinates DAG execution from the compiled `ExecutionGraph`. It is topology-driven, state-machine-based, and supports both local and distributed execution.

**Location:** `src/scheduler/`
**Key types:** `Scheduler` trait, `DefaultScheduler`, `WorkQueue`, `DistributedScheduler`

## Components

### Scheduler Trait (`src/scheduler/mod.rs`)

Core interface for all scheduler implementations. Defines `schedule()` and lifecycle methods.

### DefaultScheduler (`src/scheduler/default.rs`)

The primary local scheduler implementation. Manages a single `ExecutionGraph` through its complete lifecycle:
- Receives compiled `ExecutionGraph` from compiler
- Drives node execution via `WorkQueue`
- Handles all DAG control flow (conditional, loop, split, join, barrier)

### WorkQueue (`src/scheduler/work_queue.rs`)

Topological work queue for DAG scheduling:
- Tracks node dependencies (in-edge count)
- Returns ready nodes (all dependencies satisfied)
- Handles edge activation/deactivation
- Manages completion callbacks

### DistributedScheduler (`src/scheduler/distributed.rs`)

Extends scheduling across remote worker pools:
- `RemoteWorkerPool` — manages remote worker connections
- Falls back to `DefaultScheduler` when no remote workers available
- Node affinity hints influence scheduling decisions

### Connector Resolution

| Component | File | Purpose |
|-----------|------|---------|
| `ConnectorResolver` | `connector_resolver.rs` | Late-binding of connector instances at execution time (ADR-025) |
| `ConnectorSubscriber` | `connector_subscriber.rs` | Subscribes to connector event streams |
| `ConnectorHealth` | `connector_health.rs` | Health monitoring for connectors |

## DAG Execution

The scheduler drives the DAG state machine:

```
Pending → Running → Succeeded
                  → Failed (retryable → Pending)
                  → Failed (terminal)
```

### Control Flow Handling

| Construct | Behavior |
|-----------|----------|
| Conditional | Evaluate condition → activate matching outgoing edge |
| Loop | Boolean check → re-enqueue body or activate exit edge |
| Split | Fan-out to parallel paths |
| Join | Countdown barrier → activate when all paths arrive |
| Barrier | Manual synchronization point |

## Key Invariants

- Scheduler is topology-driven from `ExecutionGraph`
- Scheduler never mutates the `ExecutionGraph` (it is frozen)
- Scheduler owns output selection (ADR-017)
- Each node is executed exactly once (or retried per policy)
- Concurrency is bounded by available resources

## Related ADRs

- ADR-004: Topology-driven work queue scheduler
- ADR-017: Execution Runtime ABI (scheduler loop, output selection)
- ADR-025: Connector abstraction (late binding at execution)
