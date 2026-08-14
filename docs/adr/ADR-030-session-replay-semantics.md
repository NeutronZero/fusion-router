# ADR-030: Session & Replay Semantics

* **Status**: Accepted (Frozen)
* **Date**: 2026-07-26
* **Authors**: Antigravity Core Team
* **Deciders**: Architecture Review Board

---

## Context and Problem Statement

Following the completion of runtime execution (ADR-029) and policy compilation (Phase 4), FusionRouter requires execution continuity guarantees—the ability to persist session state, record atomic checkpoints, resume interrupted executions, and replay execution history deterministically across storage backends (`InMemorySessionStore` and `SqliteSessionStore`).

---

## Decision Outcomes

### 1. Separation of Identity and Snapshot

Execution continuity MUST decouple static session identity from transient execution state:
- `ExecutionSession`: Holds immutable identity (`SessionId`), workflow identifier, creation timestamp, ownership, and configuration.
- `SessionSnapshot`: Holds transient runtime state (current node, `ExecutionState`, `ExecutionContext` reference, checkpoint timestamp, trace ID).

---

### 2. Explicit Replay Modes

The `ReplayEngine` MUST operate in one of three explicit, non-overlapping `ReplayMode` settings:

1. **`Deterministic`**: Reproduces execution step-by-step using stored execution inputs and environment context.
2. **`Inspection`**: Reconstructs state transitions from recorded `ExecutionEvent` logs without invoking physical connectors or executing side effects.
3. **`Simulation`**: Re-executes workflow graph using mock connector stubs.

---

### 3. Compatibility Validation on Resume

Prior to resuming a paused or checkpointed session, `ResumeEngine` MUST validate compatibility:
- `compiler_version` match.
- `plugin_api_version` match (`v0.1.0`).
- Execution semantics version match (`ADR-029`).

If any version check fails, resumption MUST abort with a typed `CompatibilityError`.

---

### 4. Checkpoint Idempotence & Minimal Store Surface Area

- **Checkpoint Idempotence**: Persisting identical session snapshots multiple times MUST yield identical checkpoint representations without duplicate state side effects.
- **Store Isolation**: Interchanging `InMemorySessionStore` and `SqliteSessionStore` MUST NOT alter observable runtime behavior.
- **Minimal Trait Contract**: `SessionStore` MUST only define core persistence methods (`create_session`, `load_session`, `save_snapshot`, `list_checkpoints`, `delete_session`). Higher-level orchestration belongs to `SessionManager`.

---

## Consequences

- **Positive**: Guarantees deterministic session replay and crash recovery across distributed deployments.
- **Positive**: `Inspection` replay enables zero-side-effect auditing and execution visualization.
- **Negative**: Adds version verification checks to session resumption.
