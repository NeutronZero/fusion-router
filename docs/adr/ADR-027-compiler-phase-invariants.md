# ADR-027: Compiler Phase Invariants

- **Status**: Accepted
- **Date**: July 2026
- **Context**: FusionRouter v0.10.0 Compiler Architecture Constitution
- **Deciders**: FusionRouter Core Architecture Team

---

## Context

As FusionRouter transforms into a compiler-oriented capability platform, maintaining architectural purity requires strict phase boundaries. Without explicit rules, logic can bleed across phases (e.g. planner binding concrete connectors or policy compiler executing side effects).

This document serves as the **compiler constitution**, defining explicit "May Do" and "Must Not Do" constraints for every phase of the pipeline.

---

## Decisions

### Compiler Phase Matrix

| Phase | May Do | Must Not Do |
|---|---|---|
| **Plugin Manager** | Discover plugins, validate manifests, run compatibility checks, freeze registry | Mutate registry after startup, execute workflow graphs |
| **Capability Resolver** | Resolve contracts, build `CapabilityGraph`, instantiate `CapabilityInstance` handles, query cache | Execute capability logic, rewrite workflow intent |
| **Planner** | Analyze user intent, extract requirements, construct abstract `PrimitiveGraph` IR | Bind concrete connectors, execute tools, evaluate security approvals |
| **Policy Compiler** | Parse policy declarations, compile `PolicyIR`, rewrite `PrimitiveGraph` (inserting `ApprovalNode` / `PolicyGuardNode`) | Schedule graph execution, perform LLM calls, bypass security rules |
| **Optimization Passes** | Apply graph transformations (dead node elimination, fan-out consolidation), annotate `NodeMetadata` | Alter execution semantics, introduce unvetted user intent |
| **Scheduler** | Lower `PrimitiveGraph` to `ExecutionGraph`, resolve readiness dependencies, dispatch work items | Rewrite graph topology, mutate capability contracts |
| **Connector Resolver** | Perform late binding of abstract `CapabilityInstance` handles to concrete `Connector` implementations | Modify graph node ordering, alter user security policies |
| **Plugin Executor** | Execute physical node logic (Rust native / WASM / dynamic libraries), emit telemetry | Mutate workflow graph structure, bypass node metadata bounds |

---

## Consequences

- Any PR or feature addition that violates phase invariants will be rejected during code review.
- Ensures total architectural stability and determinism as the contributor base grows.
