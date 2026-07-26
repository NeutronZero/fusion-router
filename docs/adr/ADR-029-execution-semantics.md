# ADR-029: Execution Semantics & Runtime Guarantees

* **Status**: Accepted (Frozen)
* **Date**: 2026-07-26
* **Authors**: Antigravity Core Team
* **Deciders**: Architecture Review Board

---

## Context and Problem Statement

Phases 1 through 3 established the core execution substrate of FusionRouter (`CapabilityContract`, `CapabilityInstance`, `ConnectorResolver`, `ExecutionContext`, `CapabilityExecutor`, `ExecutionTrace`).

To ensure total determinism, reliability, and security across distributed environments, FusionRouter requires explicit, formal runtime execution semantics defining state machine invariants, retry guarantees, cancellation mechanics, timeout semantics, idempotency, and provenance event emission rules.

---

## Decision Outcomes

### 1. State Machine Lifecycle Invariants

The `ExecutionState` lifecycle MUST adhere to deterministic, directed transitions:

```text
               ┌──────────┐
               │ Pending  │
               └────┬─────┘
                    │
                    ▼
               ┌──────────┐
               │ Resolved │
               └────┬─────┘
                    │
                    ▼
               ┌──────────┐
               │Scheduled │
               └────┬─────┘
                    │
                    ▼
               ┌──────────┐
               │ Running  │
               └────┬─────┘
          ┌─────────┼─────────┬──────────┐
          │         │         │          │
          ▼         ▼         ▼          ▼
     ┌─────────┐┌────────┐┌─────────┐┌──────────┐
     │Succeeded││ Failed ││Cancelled││ TimedOut │
     └─────────┘└────────┘└─────────┘└──────────┘
```

- **Terminal States**: `Succeeded`, `Failed`, `Cancelled`, `TimedOut` are strictly immutable once reached.
- **State Monotonicity**: State transitions MUST move strictly forward. A terminal node MUST NOT transition back to `Running` or `Pending`.

---

### 2. Event Emission Rules & Append-Only Provenance

Every capability execution MUST emit a deterministic sequence of `ExecutionEvent` records:

1. `ConnectorBound`: Emitted upon `ConnectorResolver::bind()`.
2. `ExecutionStarted`: Emitted when `CapabilityExecutorEngine::execute_capability()` begins execution.
3. `PluginInvoked`: Emitted prior to calling physical plugin `CapabilityExecutor::execute()`.
4. `PluginCompleted`: Emitted upon physical plugin execution return.
5. `ExecutionFinished`: Emitted upon entering a terminal `ExecutionState`.

**Provenance Invariant**: `ExecutionTrace` event streams are strictly append-only. Historical events CANNOT be deleted, modified, or reordered.

---

### 3. Execution Result & Trace Decoupling

`ExecutionResult` and `ExecutionTrace` MUST remain decoupled:
- `ExecutionResult`: Business outputs (`serde_json::Value`) and execution metrics (`HashMap<String, f64>`). Never leaks internal connector handles or trace logs.
- `ExecutionTrace`: Provenance audit log (`Vec<ExecutionEvent>`). Tracks execution history for replay, telemetry, and debugging.

---

### 4. Cancellation & Timeout Guarantees

- **Cancellation**: When a cancellation signal is received, the `ExecutionContext` state MUST transition to `Cancelled`, and execution MUST abort cleanly without side effects.
- **Timeouts**: When an execution deadline (`deadline_ms`) expires prior to completion, the state MUST transition to `TimedOut` and emit an `ExecutionFinished` event with `ExecutionState::TimedOut`.

---

### 5. Idempotency & Determinism

For any capability marked as pure or deterministic:
- Executing `capability.execute(context, inputs)` \(N\) times with identical inputs MUST produce identical outputs, state transitions, and event sequence counts.

---

## Consequences

- **Positive**: Establishes formal execution guarantees equivalent to ADR-027 compiler phase invariants.
- **Positive**: Guarantees safe distributed execution, replay debugging, and reliable session orchestration in Phase 5.
- **Negative**: Adds explicit invariant enforcement checks in runtime test suites.
