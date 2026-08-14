# ADR-005: Studio API Gateway & DTO Mapping

## Status
Accepted (AF-003 Frozen)

## Context
Fusion Studio UI requires access to provider configuration, chat verification, analytics, and diagnostics without coupling the frontend to internal Rust compiler structs.

## Decision
Establish `fusion-studio-api` as an unprivileged client application gateway exposing `/api/v1/*` REST and `/ws/events` WebSocket routes. Decouple internal compiler types using explicit `Request DTO`, `Response DTO`, and `Event DTO` types in `fusion-api-public`.

## Alternatives Considered
- Direct GraphQL endpoint: Rejected to maintain lightweight Axum REST endpoints and simple client code generation.
- Serving internal compiler structs directly: Rejected to enforce Law 11 (State Decoupling) and Law 16 (Stable Contracts).

## Consequences
- UI can evolve independently from core compiler data structures.
- Studio accesses the platform through identical public APIs available to CLI and SDK clients.
