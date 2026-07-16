# ADR-006: DAG Control Flow Support (Conditionals, Loops, Split/Join)

## Status
Accepted

## Context
FusionRouter currently supports linear DAGs (nodes with simple dependencies, no branching or iteration). To unlock agentic workflows (multi-step planning, iterative refinement, parallel execution, decision branches), the system must support:

- **Conditionals** — branch execution based on a boolean condition
- **Loops** — repeat a body of nodes until a condition is met
- **Split/Join** — fan-out to parallel paths and synchronize at a join point
- **Barriers** — synchronization points for concurrent paths

## Decision

### 1. New IR Node Kinds

Four new variants added to `IRNodeKind`:

- `Conditional` — evaluates a condition expression/tool call, activates one outgoing edge based on result
- `Loop` — orchestrates repeated execution of its body subgraph; checks condition on each iteration
- `Split` — no-op fan-out node; activates all outgoing edges simultaneously
- `Join` — synchronization node; becomes ready only when all incoming edges have completed
- `Barrier` — similar to Join but emits a completion signal; used for ordering guarantees

### 2. New Execution Node Kinds

Corresponding variants added to `ExecutionNodeKind` with same semantics.

### 3. Conditional Execution

- A `Conditional` node evaluates its condition (via a tool call or a pre-configured expression stored in `config`)
- On completion, it produces a boolean result
- Its outgoing `ExecutionEdge` carries an optional `condition` string:
  - `"true"` or `"false"` for boolean matching
  - If no condition is set on an edge, it is always activated
  - If no edge matches the condition, the default/fallback edge is taken

### 4. Loop Execution

- A `Loop` node has:
  - `max_iterations` in config (safety limit, default 10)
  - Its body nodes are connected via edges: `Loop → BodyNode → … → BodyEnd → Loop`
  - The back-edge is marked with `condition: "loop"` on `ExecutionEdge`
- Scheduler handles loops:
  - First iteration starts when the Loop node becomes ready
  - After body execution completes, the WorkQueue detects the loop-back edge
  - If `iteration < max_iterations`, body nodes are reset to `Pending` and re-enqueued
  - If condition evaluates false or `iteration >= max_iterations`, the exit edge is followed

### 5. Split/Join Execution

- `Split` node: no-op, becomes `Succeeded` immediately; its outgoing edges are activated normally by the WorkQueue
- `Join` node: the WorkQueue's dependency tracking (all incoming edges must be completed) naturally implements join semantics — no special handling needed
- `Barrier` node: similar to Join but also serves as a scheduling boundary

### 6. Compiler Pass

`ControlFlowValidationPass` validates:
- Conditional nodes have outgoing edges with `condition` values
- Loop nodes have exactly one loop-back edge and one exit edge
- No cycles exist outside of loop back-edges
- Split/Join nodes are properly paired
- Barrier nodes have at least one incoming and one outgoing edge

### 7. Existing Pass Compatibility

- `ConstraintValidationPass` — unchanged, handles empty IR
- `ModelResolutionPass` — skips non-LLM nodes (Conditional, Loop, Split, Join, Barrier)
- `BudgetOptimisationPass` — skips control nodes when estimating cost

## Consequences

- The scheduler and work queue handle most control flow logic; the executor only evaluates conditions
- Loop iteration counts prevent infinite loops
- Split/Join parallelism is naturally supported by the existing topological execution model
- The planner can now produce complex DAGs with branching and iteration
- New golden tests verify each control structure independently
