# Workflow IR Specification

The Workflow IR is a high-level intermediate representation produced by the Planner and consumed by the Compiler.

## Structure

- `plan_id`: UUID identifying the plan
- `nodes`: List of IR nodes
- `edges`: Directed edges with optional conditions
- `metadata`: Policy info, cost/token estimates

## Node Kinds (Phase 8 – DAG)

| Kind | Purpose | Control Flow |
|------|---------|-------------|
| Generate | Primary LLM generation | Sequential |
| Review | Quality review of output | Sequential |
| Judge | Compare/select from multiple outputs | Sequential |
| Transform | Non-LLM data transformation | Sequential |
| Gate | Conditional routing | Sequential |
| **Conditional** | Branch based on boolean condition | **Branching** |
| **Loop** | Repeated execution of body subgraph | **Iteration** |
| **Split** | Fan-out to parallel paths | **Parallel** |
| **Join** | Synchronise parallel paths | **Parallel** |
| **Barrier** | Synchronisation point | **Parallel** |

### Conditional Nodes

- Must have at least one outgoing edge with a `condition` string (`"true"` / `"false"`)
- The Conditional node evaluates the condition and activates the matching edge
- Unmatched edges remain inactive

### Loop Nodes

- Must have `max_iterations` in config (safety limit)
- Body nodes are connected via a loop-back edge with `condition: "loop"`
- Exit edge should have `condition: "exit"`
- Scheduler enforces iteration count; body resets on each iteration

### Split / Join Nodes

- Split must have ≥2 outgoing edges (fan-out)
- Join must have ≥2 incoming edges (synchronisation)
- All split outgoing edges activate simultaneously
- Join waits for all incoming edges before becoming ready

## Strategy Kinds

| Strategy | Behavior |
|----------|----------|
| Single | One LLM call |
| Consensus | N parallel calls + judge |
| Reflection | Generate → Review → (conditional) Regenerate |
| Chain | Sequential LLM calls |
| Debate | Multiple LLMs debate |
| Fusion | Panel of judges |
