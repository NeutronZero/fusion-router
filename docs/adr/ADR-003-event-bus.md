# ADR-003: Domain Event Bus & Replay Contract

## Status
Accepted (AF-003 Frozen)

## Context
Subsystems across FusionRouter (Telemetry, Replay, Jobs, Studio UI) require decoupled, real-time access to state transitions without polling or direct inter-service coupling.

## Decision
Implement a central `DomainEvent` bus in `fusion-kernel` delivering strongly-typed events (`ExecutionStarted`, `NodeStarted`, `NodeFinished`, `Retry`, `ExecutionCompleted`, `JobUpdated`). Every event implements the `DomainEvent` trait with `id()`, `occurred_at()`, `aggregate()`, and `version()`.

## Alternatives Considered
- Direct callback hooks on handlers: Rejected due to tight coupling and inability to support asynchronous WebSocket streaming.
- External Kafka/NATS broker: Rejected for local v0.14 deployment to maintain single-binary zero-dependency installation.

## Consequences
- Studio UI receives live execution updates over WebSockets.
- Telemetry and Replay engines record and reconstruct execution trajectories deterministically.
