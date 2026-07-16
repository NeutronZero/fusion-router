# Execution Graph Specification

The Execution Graph is a compiled, executable form of the Workflow IR.

## Structure

- `graph_id`: UUID
- `nodes`: List of execution-ready nodes with resolved models, retry policies, fallbacks
- `edges`: Directed dependency edges with optional conditions
- `metadata`: Estimated cost, tokens, depth, node count

## Node Properties

- `id`: UUID
- `kind`: LLMGenerate, LLMReview, LLMJudge, Transform, Gate, Aggregate, **Conditional, Loop, Split, Join, Barrier**
- `strategy`: How this node is expanded during execution
- `model`: Resolved model name (empty for control flow nodes)
- `retry_policy`: Max retries and backoff
- `fallback`: Optional fallback model/provider
- `config`: Provider-specific configuration

## Node Kinds (Phase 8 – DAG)

| Kind | Execution Behaviour |
|------|-------------------|
| LLMGenerate | Calls the provider's chat completion |
| LLMReview | Calls the provider (review prompt) |
| LLMJudge | Calls the provider (judge prompt) |
| Transform | Non-LLM data transformation |
| Gate | Conditional routing |
| Aggregate | Merges multiple outputs |
| **Conditional** | Evaluates condition (via `config` or tool call); result determines which outgoing edge activates |
| **Loop** | Evaluates bool condition; body re-enqueues if true, exit edge activates if false |
| **Split** | No-op — becomes Succeeded immediately; all outgoing edges activate |
| **Join** | No-op — waits for all incoming edges per WorkQueue dependency tracking |
| **Barrier** | No-op — same as Join; serves as scheduling boundary |

## Edge Conditions

- `ExecutionEdge.condition`: `Option<String>`
  - `None` — always activated when source completes (normal dependency)
  - `"true"` / `"false"` — Conditional routing
  - `"loop"` — loop-back edge (not auto-activated; handled by scheduler)
  - `"exit" — exit edge from a Loop node
