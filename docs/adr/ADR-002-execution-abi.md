# ADR-002: Execution ABI & Transport Protocols

## Status
Accepted (AF-003 Frozen)

## Context
Distributed worker nodes must execute compiled DAG nodes without tight coupling to Rust memory layouts or specific transport protocols (HTTP/gRPC/WebSockets).

## Decision
Define `Execution ABI v1` in `fusion-worker-protocol` as a transport-agnostic serialization format wrapping node execution payloads, inputs, outputs, and evidence streams.

## Alternatives Considered
- Direct gRPC Protobuf binding: Rejected as primary contract to allow lightweight HTTP/WebSocket transport in single-binary local deployments.
- In-process dynamic libraries: Rejected for distributed workers to ensure crash isolation and language-agnostic worker nodes.

## Consequences
- Workers communicate via `Execution ABI v1`.
- Enables distributed Coordinator/Worker clusters and transport evolution (HTTP -> gRPC -> QUIC) without modifying runtime semantics.
