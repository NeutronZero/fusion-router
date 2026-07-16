# Runtime Architecture

FusionRouter processes every chat completion request through a deterministic pipeline from ingress to egress. Each stage is independently testable and swappable.

## Pipeline

```
Request
  │
  ▼
┌──────────────┐
│  Context     │  assemble messages, files, tools into ContextSnapshot
│  Assembler   │
└──────┬───────┘
       ▼
┌──────────────┐
│ Requirements │  classify intent (Code/Debug/Analysis/…) & complexity (Low→Critical)
│ Extractor    │
└──────┬───────┘
       ▼
┌──────────────┐
│  Planner     │  produce WorkflowIR (DAG of IR nodes with edges & conditions)
│              │  may produce: linear │ split/join │ conditional │ loop graphs
└──────┬───────┘
       ▼
┌──────────────┐
│  Compiler    │  run validation → control flow → model resolution → budget passes
│              │  lower to ExecutionGraph (resolved models, retries, fallbacks)
└──────┬───────┘
       ▼
┌──────────────┐
│  Scheduler   │  topological execution with:
│              │   • conditional branching (activate matching edge)
│              │   • loop iteration (re-enqueue body up to max_iterations)
│              │   • split/join parallelism (fan-out + dependency sync)
│              │   • retry & fallback on failure
└──────┬───────┘
       ▼
┌──────────────┐
│  Executor    │  resolve strategy → expand subgraph → invoke providers
│              │  control nodes: conditional, loop, split, join, barrier
└──────┬───────┘
       ▼
┌──────────────┐
│  Telemetry   │  record execution evidence to SQLite
└──────┬───────┘
       ▼
   Response
```

## DAG Execution Model

### Conditional
```
[Cond] ──"true"──▶ [BranchA]
   │
   └──"false"──▶ [BranchB]
```
The Conditional node evaluates a condition (via config or tool call) and activates **only** the matching outgoing edge. The scheduler uses `mark_conditional_completed` + `activate_edge` to prevent the non-matching branch from running.

### Loop
```
[Loop] ──(body)──▶ [Body1] ▶ [Body2] ──"loop"──▶ [Loop]
   │
   └──"exit"──▶ [Continue]
```
The Loop node checks its boolean output. If `true`, the body nodes are reset to Pending and re-enqueued. If `false` (or `max_iterations` reached), the exit edge activates. Loop-back edges (`condition: "loop"`) are never auto-activated — only the scheduler activates them.

### Split / Join
```
[Split] ──▶ [TaskA] ──▶ [Join]
   │                    ▲
   └───────▶ [TaskB] ───┘
```
Split is a no-op that fans out to all outgoing edges. Join waits for all incoming edges (enforced by WorkQueue's dependency tracking). Parallel tasks execute concurrently via `join_all`.

### Barrier
```
[TaskA] ──▶ [Barrier] ──▶ [TaskC]
[TaskB] ──▶  (sync)
```
Barrier synchronises concurrent paths before continuing. Same semantics as Join, but serves as a scheduling boundary rather than a data merge point.

## Scheduling Algorithm

1. Query WorkQueue for ready nodes (all dependencies met + edges activated)
2. Mark ready nodes as `Running`
3. Execute all ready nodes concurrently via `join_all`
4. For each completed node:
   - **Normal**: `mark_completed` → auto-activates outgoing edges
   - **Conditional**: `mark_conditional_completed` → activate matching edge only
   - **Body node with loop-back**: increment iteration → if under limit, reset body; else exit
   - **Failed**: retry with backoff or fallback
5. Repeat until all nodes are `Succeeded` or `Failed`

## Parallelism

- Split/Join enables arbitrary fan-out within a single request
- All ready nodes execute concurrently (bounded by `max_concurrent` via ResourceManager)
- Join nodes naturally synchronise via dependency tracking
