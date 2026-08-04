# ADR-006: Worker Discovery & Federation

## Status
Accepted (AF-003 Frozen)

## Context
As execution expands from single-binary local deployments to distributed worker nodes, workers must dynamically register capabilities with the Coordinator.

## Decision
Implement a self-registration discovery protocol in `fusion-worker-protocol` (`Hello -> Capabilities -> Heartbeat -> Coordinator`) with health probes and resource tracking.

## Alternatives Considered
- Static IP configuration of workers: Rejected due to inability to support auto-scaling or dynamic worker topologies.

## Consequences
- Coordinator schedules work dynamically based on advertised worker capabilities and capacity.
