# ADR-016: Intent-Oriented Execution Model

**Status:** Approved  
**Date:** 2026-07-18  
**Supersedes:** None  

---

## Context

FusionRouter has evolved from a simple model router into an orchestration engine that compiles workflows into executable DAGs. The current public API (`/v1/chat/completions`) is OpenAI‑compatible and accepts parameters like `model`, `messages`, `tools`, and `stream`. This has served well for basic usage.

However, as we expand the planner to support multi‑step workflows, consensus, reflection, and other orchestration patterns, we face a choice:

- Expose each pattern as a separate API parameter (e.g., `"strategy": "consensus"`).
- Or expose a higher‑level expression of *what the client wants*, and let the planner determine *how* to achieve it.

The first approach risks coupling the public API to specific implementation techniques that may evolve or become obsolete. The second approach preserves the ability to improve the planner without breaking clients.

---

## Decision

We adopt an **intent‑oriented execution model** for FusionRouter's public API.

### Core Principle

> **Clients express execution intent, not execution mechanics. The planner compiles intent into an internal `ExecutionGraph`, which is an implementation detail and not part of the public contract.**

### Architectural Invariant

> **The public API MUST NOT expose planner implementation details, execution graph structure, scheduling algorithms, or orchestration techniques as required request parameters.**

This invariant protects the API from becoming entangled with specific planning heuristics. The planner is free to change its internal algorithms, introduce new strategies, or even replace the entire compilation pipeline without requiring clients to update their requests.

### API Implications

- The `/v1/chat/completions` endpoint will accept an optional `execution` field that describes desired **characteristics**, not concrete orchestration steps.
- The field may express:
  - A **policy preference** (e.g., `mode: "quality"`, `mode: "speed"`, `mode: "balanced"`).
  - **Explicit constraints** (e.g., `max_latency_ms`, `max_cost_usd`).
  - **Output preferences** (e.g., `include_report: true`).
- The field **must not** specify:
  - Which models to use.
  - Whether to use consensus, reflection, or a single model.
  - The structure of the execution graph.

### Policy vs. Intent

Named execution modes like `quality`, `speed`, and `balanced` are **convenience policies**. They are **not** an architectural primitive. They are examples of how a client might express intent without providing low‑level constraints. The planner maps these policy names to specific graph configurations (e.g., `quality` → consensus + judge + reflection), but that mapping is:

- Subject to change without API versioning.
- Not part of the public contract.

Clients that require deterministic or reproducible execution should either rely on the stable policy semantics or use explicit constraints (e.g., `max_latency_ms`, `max_cost_usd`) to define their intent more precisely. The policy names are guaranteed to be stable only in their *intended effect* (e.g., "quality" implies careful deliberation), not in the precise graph they produce.

### Internal Architecture

```
Client Intent
        │
Requirements Extraction
        │
Execution Planning (Planner)
        │
ExecutionGraph (compiled DAG)
        │
Optimization (budget, fusion, scheduling hints)
        │
Scheduling
        │
Execution
        │
Response + Artifacts
```

### Reporting

Every execution produces a set of artifacts that explain *what happened*:

- **Response**: the final answer.
- **Report**: costs, timing, model breakdown, graph decisions.
- **Trace**: timeline of each step.
- **Artifacts**: additional data (e.g., visualisations, debug info).

These artifacts are accessible via separate endpoints (e.g., `GET /v1/executions/{id}/report`) and are **not** part of the response body to keep the chat completion response lightweight.

### Stability Guarantee

> **The mapping from execution intent to execution graph is intentionally unspecified by the public API and may change across releases without constituting a breaking API change.**

This guarantee is the essence of the decision. It enables planner innovation without requiring clients to adapt.

---

## Consequences

### Positive

- **API Stability**: The public API remains stable even as the planner evolves.
- **Planner Independence**: Internal improvements (new algorithms, better model selection) do not require client changes.
- **Client Clarity**: Clients express *what they want*, not *how to do it*.
- **Easier Adoption**: Users don't need to understand consensus, reflection, etc. to use FusionRouter effectively.

### Negative

- **Planner Complexity**: The planner must interpret intent and produce optimal graphs, which is more complex than simple routing.
- **Less Surface for Experimentation**: It may be harder for advanced users to force a specific strategy for testing or benchmarking.
- **Requires Clear Documentation**: The meaning of each execution mode must be clearly documented and stable in intent, even if the graph changes.

### Mitigations

- Internal debugging and testing facilities may expose additional execution controls. Such facilities are explicitly **outside the public API contract** defined by this ADR.
- The `report` artifact provides full transparency into *how* the intent was executed, satisfying curiosity and debugging needs.

---

## Examples

| Client Intent       | Planner Compiles To (Illustrative, Subject to Change) |
|---------------------|------------------------------------------------------|
| `mode: "quality"`   | Consensus (3 models + judge) → Reflection → Final Answer |
| `mode: "speed"`     | Single model, no deliberation                        |
| `mode: "balanced"`  | Consensus (2 models + judge)                         |
| `mode: "exhaustive"`| Multiple consensus rounds + cross‑check + reflection + final synthesis |
| `max_latency_ms: 1000` | Single model, no deliberation (if necessary to meet latency) |

These mappings are **examples only**. The planner may adjust them without notice.

---

## Status

- [x] Proposed
- [x] Approved
- [ ] Implemented (v0.8.0)

---

## References

- ADR-003 (Compiler)
- ADR-007 (Workflow Registry)
- Architecture Specification: §4.4 (Planner), §4.5 (Compiler)
