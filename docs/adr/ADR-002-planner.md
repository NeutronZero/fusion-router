# ADR-002: Planner

## Status
Accepted

## Context
The Planner transforms requirements into a workflow plan. It must be policy-driven and evidence-informed.

## Decision
1. **Planner produces WorkflowIR, not ExecutionGraph**: The IR is a high-level plan; compilation is a separate concern.
2. **Policy evaluation**: Policies are evaluated in priority order against requirements.
3. **Evidence-informed**: An optional `EvidenceSnapshot` biases model and strategy selection based on historical success.
4. **Default planner**: A `SimplePlanner` uses keyword-based intent classification and complexity-based strategy selection.

## Consequences
- Clean separation between planning and compilation.
- Policies can be hot-reloaded from config.
- Evidence integration is optional and non-breaking.
