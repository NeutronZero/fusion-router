# ADR-004: Scheduler

## Status
Accepted

## Context
The Scheduler executes an ExecutionGraph, managing node state transitions, retries, and fallbacks.

## Decision
1. **Topology-driven**: A node executes when all dependency nodes have succeeded (based on `ExecutionEdge.from`).
2. **Work queue**: Maintains a queue of nodes whose dependencies are satisfied.
3. **State machine**: Each node transitions through Pending → Running → Succeeded/Failed.
4. **Retry policy**: Nodes carry a `RetryPolicy` (max_retries, backoff_ms). The scheduler retries on failure.
5. **Fallback**: If all retries fail and a `FallbackConfig` exists, the scheduler attempts execution on the fallback model/provider.

## Consequences
- Deterministic execution order.
- Graceful degradation via retries and fallbacks.
- Clean separation from executor logic.
