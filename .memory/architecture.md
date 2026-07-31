# FusionRouter Architecture

## System Architecture

FusionRouter follows a **compiler pipeline** architecture with discrete stages connected by well-defined IR boundaries.

### Request Lifecycle

```
Client Request
    │
    ▼
Server (Axum HTTP)
    │  Middleware: Auth → CORS → Rate Limit → Request ID
    ▼
ContextAssembler
    │  Builds ContextSnapshot(system prompt, history, tools)
    ▼
RequirementsExtractor
    │  Classifies intent, extracts complexity
    ▼
Planner
    │  WorkflowPlanner → SimplePlanner (fallback) → DynamicPlanner (optional)
    ▼
    WorkflowIR
    ▼
Compiler
    │  Pass pipeline (7 standard passes + optimizations)
    │  PrimitiveGraph → ExecutionGraph derivation
    ▼
    ExecutionGraph
    ▼
Scheduler
    │  WorkQueue DAG scheduler
    ▼
Executor
    │  CapabilityExecutor → Providers/Strategies/Tools/Connectors
    ▼
    ExecutionResult
```

### Data Models

| Type | Stage | Description |
|------|-------|-------------|
| `ContextSnapshot` | After Context Assembly | System prompt, conversation history, tool definitions |
| `Requirements` | After Requirements Extraction | Intent classification, complexity score |
| `WorkflowIR` | After Planning | Abstract plan: nodes (Generate, Review, Judge, Transform, Gate, Conditional, Loop, Split, Join, Barrier), edges with conditions |
| `PrimitiveGraph` | After Lowering | Canonical lowered IR, strategy-expanded subgraphs |
| `ExecutionGraph` | After Compilation | Executable form: 12 node kinds, resolved models, retry policies, deterministic UUIDs |
| `ExecutionInstance` | During Execution | Bound node + runtime params |
| `ExecutionResult` | After Execution | Standardized output + metrics |

## Compiler Pipeline (7 Standard Passes)

1. **Constraint Validation** — Validates WorkflowIR invariants: cycles, entry/exit points, edge types
2. **Capability Resolution** — Resolves capability references to concrete `CapabilityInstance` bindings
3. **Budget Optimization** — Applies resource budgets from policy
4. **Node Fusion** — Merges compatible sequential nodes
5. **Retry & Fallback Insertion** — Wraps nodes with retry/fallback logic from policy
6. **Scheduling Hints** — Annotates nodes with scheduling metadata
7. **Graph Verification** — Validates final ExecutionGraph invariants

## Strategy Types

| Strategy | Description | Source |
|----------|-------------|--------|
| Single | Single LLM call | `src/strategies/single.rs` |
| Consensus | Multiple independent calls, vote | `src/strategies/consensus.rs` |
| Reflection | Generate → critique → refine loop | `src/strategies/reflection.rs` |
| Debate | Multi-agent debate framework | `src/strategies/debate.rs` |
| ReAct | Reasoning + Action loop | `src/strategies/react.rs` |
| Chain | Sequential chained calls | `src/strategies/chain.rs` |
| Fusion | Parallel calls with synthesis | `src/strategies/fusion.rs` |

## Architectural Invariants (16 Core)

From `docs/architecture/invariants.md`:

1. LLM interactions go through the `Provider` trait
2. Compiler is a series of pure passes — no side effects, no LLM calls
3. Planner produces `WorkflowIR`, not `ExecutionGraph`
4. Scheduler is topology-driven from the `ExecutionGraph`
5. All subgraphs have exactly one entry and one exit point
6. Capability resolution is late-bound at compilation time
7. Resource cleanup is guaranteed through RAII (`ResourceGuard`)
8. Evidence is written after execution, not during
9. Configuration is validated at startup, immutable at runtime
10. Telemetry never blocks request processing
11. Plugin discovery happens at startup only
12. The `ExecutionGraph` is frozen after compilation — never mutated by runtime
13. Compilation is deterministic: same input → same `ExecutionGraph`
14. The compiler pipeline is extensible via registered passes
15. The compiler owns `ExecutionGraph` and is responsible for its lifecycle
16. ADR-027 compiler phase invariants matrix: each phase has "May Do" / "Must Not Do" rules

## Feature Flags

| Feature | Default | Gates |
|---------|---------|-------|
| `semantic-cache` | Yes | USearch HNSW semantic vector cache |
| `wasm-plugins` | No | Wasmtime WASM runtime |
| `dev-console` | No | `console-subscriber` for tokio console |
| `otel` | No | OpenTelemetry OTLP exporter |

## External Crates (FusionRouter SDK)

| Crate | Path | Purpose |
|-------|------|---------|
| `fusion-plugin-api` | `crates/fusion-plugin-api/` | Minimal SDK for capability plugins |
| `fusion-capability-macros` | `crates/fusion-capability-macros/` | Proc-macros for capability declarations |
| `fusion-capability-sdk` | `crates/fusion-capability-sdk/` | Developer SDK for building capabilities |
