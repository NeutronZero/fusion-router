# ADR-007: Storage Model & Repository Abstractions

## Status
Accepted (AF-003 Frozen)

## Context
FusionRouter needs reliable persistent storage for settings, execution trajectories, telemetry, evidence, and audit logs without tying domain services directly to SQLite SQL queries.

## Decision
Implement a single SQLite database file (`fusion_data.db`) in `fusion-infrastructure` behind repository traits (`ProviderRepository`, `ExecutionRepository`, `TelemetryRepository`, `ConfigRepository`).

## Alternatives Considered
- Multiple separate database files: Rejected due to cross-database consistency issues and backup complexity.
- Directly embedding SQL queries into API handlers: Rejected to enforce Invariant 7 (Pure Storage Repositories).

## Consequences
- Single database backup and transaction safety.
- Easy swap to PostgreSQL in enterprise editions without changing domain services.
