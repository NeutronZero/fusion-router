# FusionRouter Execution Model

## Overview

The execution model governs how compiled `ExecutionGraph` nodes are dispatched, how sessions are managed, and how replay/correctness guarantees are enforced.

**Location:** `src/executor/`, `src/session/`, `src/lifecycle/`

## Executor (`src/executor/`)

| Component | File | Purpose |
|-----------|------|---------|
| `Executor` trait | `src/executor/mod.rs` | Core executor interface |
| `DefaultExecutor` | `src/executor/mod.rs` | Standard executor implementation |
| `CapabilityExecutor` | `src/executor/capability_executor.rs` | Unified capability execution dispatch |

The executor dispatches scheduled nodes to the appropriate handler:

- **LLM nodes** → `Provider` trait (model call via transport)
- **Strategy nodes** → `Strategy` trait (multi-step reasoning subgraph)
- **Tool nodes** → `Tool` trait (built-in tools via `ToolRegistry`)
- **Connector nodes** → `Connector` trait (external service connectors)
- **Capability nodes** → `CapabilityExecutor` (unified capability dispatch)

### Subgraph Message Inheritance

Strategy sub-nodes (consensus members, judge, reviewer) are prebuilt at compile time
by `strategy_expansion` **without** the request context. To guarantee every LLM
request carries the user's input:

- `CompilationStep` (`src/server/pipeline.rs`) injects the assembled `messages`
  into every LLM node and its LLM sub-nodes.
- `DefaultExecutor::propagate_parent_messages` (src/executor/mod.rs) copies the
  parent node's non-empty `messages` into any LLM sub-node that still lacks them
  — applied to both prebuilt subgraphs and runtime-lowered legacy subgraphs.
- Regression covered by `test_resolve_strategy_propagates_parent_messages_to_subnodes`.

## Tool Execution Trust Boundary (Law 7 / ADR-037)

- **Model output is data, never commands.** The executor no longer parses
  model output text for `{"tool": ...}` JSON — a model printing a tool-shaped
  string returns it as TEXT and never executes it.
- **Execution is fed only from provider-native `tool_calls`**, surfaced on
  `ChatCompletionResponse.native_tool_calls` (structured `ToolCall { id,
  name, arguments }`), normalized per provider in `native_tool_calls_from`.
- **`DefaultExecutor.allow_auto_exec`** (config `tools.allow_auto_exec`,
  default `false`): tool calls are executed only when enabled AND the
  request names a non-empty per-request allowlist
  (`node.config["tool_allowlist"]`). Empty/absent allowlist ⇒ nothing
  executes (fail closed); non-allowlisted calls are returned as text with a
  `reason`.
- **Allowlist wiring**: `CompilationStep` copies the request's declared
  `tools` names into `node.config["tool_allowlist"]` for every LLM node and
  its LLM sub-nodes (`propagate_parent_messages` carries the allowlist into
  runtime-lowered subgraphs as a fallback). A request that declares no
  tools sends no allowlist — tools stay invisible (fail closed).
- Tool definitions are advertised to the provider (`ChatCompletionRequest.tools`)
  only when auto-exec is enabled with an allowlist — otherwise the provider
  cannot emit tool calls at all.
- Providers without native tool-call support execute no tools (no emulation).
- **Bounded tool loop (ReAct-style)**: when the model emits native tool calls
  and at least one executes, the results are appended to the conversation
  (`Tool results: <json>`) and the model is re-prompted, so it can read files,
  observe results, and continue. The loop ends when the model emits plain text
  or when the round budget (`node.config["max_tool_rounds"]`, default 8) is
  exhausted. Verified live (2026-08-08): a self code-review on OpenCode Zen
  (`deepseek-v4-flash-free`) executed 12 `file_read` calls across 3 rounds and
  produced a file-referencing review report.

## Execution State Machine (ADR-029)

```
Pending ──→ Resolved ──→ Scheduled ──→ Running ──→ Succeeded
                                              ──→ Failed (retryable → Resolved)
                                              ──→ Failed (terminal)
                                              ──→ Cancelled
```

### Event Emission

Each state transition emits an `ExecutionEventEnvelope` with monotonic sequencing (ADR-017). Events are immutable after emission.

### Guarantees

| Property | Mechanism |
|----------|-----------|
| Cancellation | Token-based cancellation propagation |
| Timeouts | Per-node timeout enforcement, `BudgetEnvelope` |
| Idempotency | Deterministic UUIDs on all graph nodes |
| Retry | Configurable retry policy with exponential backoff |
| Isolation | Session-scoped execution context |

## Execution Session (ADR-026)

| Concept | Type | Description |
|---------|------|-------------|
| Identity | `ExecutionSession` | Container for a single execution run |
| Snapshot | `SessionSnapshot` | Point-in-time capture of execution state |
| Store | `SessionStore` trait | `MemorySessionStore`, `SqliteSessionStore` |

## Session Continuity (ADR-030)

### Three Replay Modes

| Mode | Behavior |
|------|----------|
| `Deterministic` | Exact replay: same inputs, same order, same results |
| `Inspection` | Read-only exploration of a past execution |
| `Simulation` | What-if analysis with modified parameters |

### Checkpoint Engine (`src/session/checkpoint.rs`)

- Configurable checkpoint intervals (node count, time, manual)
- Captures full `SessionSnapshot` at each checkpoint
- Enables replay from any checkpoint boundary

### Compatibility Validation

On session resume, validates that the runtime version, capability contracts, and provider configurations are compatible with the snapshot's recorded state.

## Trigger Framework (ADR-031)

**Location:** `src/trigger/`

| Type | Description |
|------|-------------|
| `ExecutionRequest` | Canonical request type — single-pipeline invariant |
| `TriggerTrace` | Provenance chain: trigger source → policy → execution |
| Webhook | Incoming HTTP-triggered executions |
| Cron | Scheduled time-based executions |
| EventBus | Internal event-driven executions |

### Invariants

- All requests follow the single-pipeline: trigger → context assembly → planner → compiler → scheduler → executor
- Request payload is immutable after reception
- Deduplication via `TriggerId` / `ProvenanceId`
- Unified provenance chain: `TriggerTrace → PolicyTrace → ExecutionTrace`

## Lifecycle Manager (`src/lifecycle/manager.rs`)

Orchestrates the full session lifecycle:
1. Session creation and initialization
2. Checkpoint placement
3. Session suspension and resumption
4. Session teardown and cleanup

## Related ADRs

- ADR-016: Intent-oriented execution (clients express intent, not mechanics)
- ADR-017: Execution Runtime ABI (scheduler loop, output selection)
- ADR-026: Execution Session (session/store decoupling)
- ADR-029: Execution Semantics (formal state machine, frozen)
- ADR-030: Session Replay Semantics (3 replay modes, frozen)
- ADR-031: Trigger Request Semantics (canonical request, frozen)
