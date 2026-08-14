# ADR-017: Execution Runtime ABI & Engine Stabilization

- **Status**: Accepted
- **Date**: July 2026
- **Context**: FusionRouter v0.8.1 Architectural Stabilization Release
- **Deciders**: FusionRouter Engineering Team

---

## Context

Prior to v0.8.1, the boundary between the compiler, scheduler, and pipeline response builder had areas of implicit coupling:
1. `DefaultScheduler` maintained two duplicate execution loops (`run` vs `run_with_cancellation`), creating risk of telemetry and budget enforcement drift.
2. `ResponseBuilderStep` selected output by calling `.last()` on an unordered `HashMap<Uuid, Value>`, which was non-deterministic across multi-node DAG executions.
3. `CompilationStep` forcibly overwrote `node.model` with the raw client request model, bypassing decisions made by `ModelResolutionPass`.

To support future engine capabilities (compiler graph caching, typed IR separation, and deterministic replay), the runtime interface between the scheduler and higher-level pipeline must be formalized as a stable contract.

---

## Decision

We formalize **`ExecutionResult`** as the authoritative **Runtime ABI** between the `Scheduler` and the `Pipeline`.

```rust
/// Runtime ABI contract between Scheduler and Pipeline.
/// Changes to this structure impact response building, telemetry, and execution reporting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub instance_id: Uuid,
    pub success: bool,
    pub outputs: HashMap<Uuid, serde_json::Value>,
    pub total_latency_ms: u64,
    pub total_cost: f64,
    pub total_tokens: u64,
    pub terminal_node_id: Option<Uuid>,
    pub final_output: Option<serde_json::Value>,
}
```

### Key Architectural Invariants

1. **Single Authoritative Scheduler Loop**:
   - `DefaultScheduler::run_inner` is the sole execution engine.
   - All entry points (`run()`, `run_with_cancellation()`) delegate directly to `run_inner()`.
   - All budget enforcement, iteration limits, metrics, and tracing calls are strictly consolidated inside `run_inner()`.

2. **Scheduler Ownership of Output**:
   - The scheduler explicitly tracks the terminal/exit node during graph traversal and populates `terminal_node_id` and `final_output`.
   - `ResponseBuilderStep` reads `final_output` directly without inferring node completion order.

3. **Compiler Ownership & Frozen Graphs**:
   - Once compilation completes, downstream pipeline stages MUST NOT modify execution semantics (model selection, graph topology, control flow, or scheduling decisions).
   - Downstream stages may only inject runtime context data (messages, credentials, telemetry).

---

## Consequences

### Positive
- **Determinism**: Multi-node DAG responses (`Generate -> Reflect -> Judge`) consistently select the output of the terminal node (`Judge`).
- **Telemetry Integrity**: Zero observability drift between synchronous and cancellable execution paths.
- **Compiler Authority**: `ModelResolutionPass` selections survive generic request inputs (`"auto"`).
- **Foundation for v0.9**: Provides a stable execution ABI necessary for compiler caching, typed IR separation, and deterministic replay.

### Replay & Evolution Policy
- Breaking changes to `ExecutionResult` require an ADR review.
- Serialization (`serde::Serialize`, `serde::Deserialize`) must remain backwards compatible for recorded execution logs to enable replayability.
