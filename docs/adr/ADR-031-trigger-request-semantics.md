# ADR-031: Trigger & Request Semantics

* **Status**: Accepted (Frozen)
* **Date**: 2026-07-26
* **Authors**: Antigravity Core Team
* **Deciders**: Architecture Review Board

---

## Context and Problem Statement

Following the completion of the Trigger Framework (Phase 6), FusionRouter requires explicit, formal guarantees governing how external triggers (Webhooks, Cron timers, EventBus messages, Manual invocations) generate execution requests and how provenance is recorded across the platform.

---

## Decision Outcomes

### 1. Canonical ExecutionRequest Invariant

All external triggers MUST convert their payloads into a single canonical `ExecutionRequest` structure:

```rust
pub struct ExecutionRequest {
    pub request_id: Uuid,
    pub trigger_kind: TriggerKind,
    pub trigger_name: String,
    pub payload: serde_json::Value,
    pub requester_identity: String,
    pub created_at_ms: u64,
}
```

**Single-Pipeline Invariant**: Every `ExecutionRequest` MUST pass through the exact same `Planner` → `CompilerPipeline` → `ExecutionGraph` → `LifecycleManager` → `Scheduler` pipeline. Direct runtime execution of raw trigger payloads without compilation is strictly FORBIDDEN.

---

### 2. Payload Immutability & Deduplication

- **Payload Immutability**: Once an `ExecutionRequest` is instantiated, its `payload` and `request_id` are strictly immutable.
- **Deduplication**: Webhook and EventBus triggers with identical correlation IDs within a configured deduplication window MUST be deduplicated before entering the `CompilerPipeline`.

---

### 3. Unified Provenance Chain

FusionRouter provenance MUST form an unbroken, multi-layer trace chain:

```text
TriggerTrace  ──►  PolicyTrace  ──►  ExecutionTrace
```

1. **`TriggerTrace`**: Captures trigger source, timestamp, payload fingerprint, and requester identity.
2. **`PolicyTrace`**: Captures policy rule evaluation matches, precedence decisions, and graph transformations.
3. **`ExecutionTrace`**: Captures runtime lifecycle state transitions, connector binding, and execution events.

---

## Consequences

- **Positive**: Eliminates separate execution paths for webhooks, cron jobs, and manual requests.
- **Positive**: Provides 100% complete end-to-end provenance across compilation and execution.
- **Negative**: Adds light serialization overhead for payload fingerprinting.
