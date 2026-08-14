# ADR-025: Connector Abstraction & Late Binding Connector Resolver

- **Status**: Proposed
- **Date**: July 2026
- **Context**: FusionRouter v0.10.0 Connector Architecture
- **Deciders**: FusionRouter Core Architecture Team

---

## Context

The Planner must remain agnostic to concrete implementation details (e.g. Gmail vs. Outlook vs. SendGrid for email, or GitHub vs. GitLab for git repos). If the Planner binds to specific connectors early, workflows lose portability and vendor independence.

---

## Decisions

### 1. Planner Agnosticism

The Planner plans purely against abstract **Capability Contracts** (e.g. `capability.send_email`, `capability.issue.create`).

### 2. Late Binding via Connector Resolver

Binding abstract capabilities to concrete `Connector` instances occurs at execution time in the Scheduler via a `ConnectorResolver`.

```text
Planner ──► Abstract Capability ("send_email")
                 │
                 ▼
          PrimitiveGraph / ExecutionGraph
                 │
                 ▼
          Scheduler / Runtime
                 │
                 ▼
          Connector Resolver (Late Binding)
                 │
  ┌──────────────┼──────────────┐
  ▼              ▼              ▼
Gmail         Outlook        SendGrid
```

Like OS system calls (`printf()` → `glibc` → kernel syscall), the workflow IR specifies the abstract interface, while the runtime binds the optimal concrete connector based on active provider configuration and authentication context.

---

## Consequences

- Workflows are vendor-agnostic and fully portable across cloud, desktop, and enterprise environments.
- Connectors can be swapped or failed over at runtime without re-planning the execution graph.
