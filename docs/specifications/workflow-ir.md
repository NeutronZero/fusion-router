# Workflow IR Specification

The Workflow IR is a high-level intermediate representation produced by the Planner and consumed by the Compiler.

## Structure

- `plan_id`: UUID identifying the plan
- `nodes`: List of IR nodes (Generate, Review, Judge, Transform, Gate)
- `edges`: Directed edges with optional conditions
- `metadata`: Policy info, cost/token estimates

## Node Kinds

| Kind | Purpose |
|------|---------|
| Generate | Primary LLM generation |
| Review | Quality review of output |
| Judge | Compare/select from multiple outputs |
| Transform | Non-LLM data transformation |
| Gate | Conditional routing |

## Strategy Kinds

| Strategy | Behavior |
|----------|----------|
| Single | One LLM call |
| Consensus | N parallel calls + judge |
| Reflection | Generate → Review → (conditional) Regenerate |
| Chain | Sequential LLM calls |
| Debate | Multiple LLMs debate |
| Fusion | Panel of judges |
