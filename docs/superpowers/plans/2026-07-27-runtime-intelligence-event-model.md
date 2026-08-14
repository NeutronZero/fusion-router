# v0.11 — Runtime Intelligence & Event-Driven Engine Implementation Plan

> **Goal:** Transform FusionRouter into an event-driven runtime by implementing canonical `ExecutionEvent` envelopes, an abstract `EventBus` trait, a decoupled `EventProjection` framework, OpenTelemetry tracing, timeline visualizer, policy-driven checkpointing, and persistent event storage.

---

## Technical Architecture & Design Invariants

- **Runtime Event Stream ABI:** The execution event stream is the official Observability ABI between the runtime engine and all downstream observability/preservation projections.
- **Delivery & Isolation Guarantees:**
  - *Ordering:* Events are published in monotonic `sequence_number` order per `execution_id`.
  - *Delivery:* `BroadcastEventBus` provides at-most-once async broadcast delivery.
  - *Isolation:* Projections execute asynchronously on background tasks; projection panics or delays never block runtime execution loops.
- **Sole Authority for Canonical Serialization:** `ExecutionEventEnvelope` carries `schema_version: "fusion.router.event.v1"`, `correlation_id`, and `parent_event_id`.
- **Projection Composition:** OpenTelemetry (`otel.rs`), Timeline (`timeline.rs`), Checkpoints (`checkpoint.rs`), and Event Store (`storage.rs`) implement `EventProjection`.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/events/mod.rs` | `ExecutionEventEnvelope`, `schema_version`, module re-exports |
| `src/events/payload.rs` | Strongly-typed `ExecutionEvent` taxonomy (lifecycle, nodes, retries, providers, tools, resources) |
| `src/events/bus.rs` | `EventBus` trait, `BroadcastEventBus` implementation |
| `src/events/projection.rs` | `EventProjection` trait, `ProjectionDispatcher` registry & fan-out engine |
| `src/events/consumers/otel.rs` | OpenTelemetry projection |
| `src/events/consumers/timeline.rs` | Millisecond-accurate timeline renderer (`fusion trace timeline`) |
| `src/events/consumers/checkpoint.rs` | Policy-driven `CheckpointEngine` (`CheckpointPolicy`) |
| `src/events/consumers/storage.rs` | SQLite/JSONL persistent event store |
| `src/bin/fusion.rs` | CLI subcommands: `fusion trace timeline <ID>` and `fusion trace events <ID>` |
| `tests/runtime_events_tests.rs` | End-to-end integration test suite |

---

## Task Breakdown & Checklists

### Task 1: Canonical Event Envelopes, Taxonomy & EventBus Trait (Sprint N1)

**Files:**
- Create: `src/events/mod.rs`
- Create: `src/events/payload.rs`
- Create: `src/events/bus.rs`

- [ ] **Step 1: Implement `ExecutionEventEnvelope` & taxonomy in `src/events/mod.rs` & `payload.rs`**
- [ ] **Step 2: Implement `EventBus` trait & `BroadcastEventBus` in `src/events/bus.rs`**
- [ ] **Step 3: Add unit tests for event serialization & bus publish/subscribe**

Run: `cargo test events::bus`

---

### Task 2: Projection Framework & Dispatcher (Sprint N1.5)

**Files:**
- Create: `src/events/projection.rs`
- Modify: `src/events/mod.rs`

- [ ] **Step 1: Implement `EventProjection` trait & `ProjectionDispatcher` in `src/events/projection.rs`**
- [ ] **Step 2: Implement isolated background fan-out dispatch loop**
- [ ] **Step 3: Add unit tests for projection registration and panic-isolation**

Run: `cargo test events::projection`

---

### Task 3: OpenTelemetry & Timeline Projections (Sprints N2 & N3)

**Files:**
- Create: `src/events/consumers/otel.rs`
- Create: `src/events/consumers/timeline.rs`
- Modify: `src/events/mod.rs`

- [ ] **Step 1: Implement OpenTelemetry projection (`otel.rs`)**
- [ ] **Step 2: Implement Timeline visualizer (`timeline.rs`) with ASCII and JSON formatters**
- [ ] **Step 3: Add unit tests for timeline rendering**

Run: `cargo test events::consumers::timeline`

---

### Task 4: Checkpointing & Persistent Storage Projections (Sprints N4 & N5)

**Files:**
- Create: `src/events/consumers/checkpoint.rs`
- Create: `src/events/consumers/storage.rs`
- Modify: `src/events/mod.rs`

- [ ] **Step 1: Implement policy-driven `CheckpointEngine` with `CheckpointPolicy` (`checkpoint.rs`)**
- [ ] **Step 2: Implement `PersistentEventStore` with JSONL/SQLite backing (`storage.rs`)**
- [ ] **Step 3: Add unit tests for checkpointing and storage persistence**

Run: `cargo test events::consumers::checkpoint` and `cargo test events::consumers::storage`

---

### Task 5: Governance CLI Integration & End-to-End Tests

**Files:**
- Modify: `src/bin/fusion.rs`
- Create: `tests/runtime_events_tests.rs`

- [ ] **Step 1: Add `fusion trace timeline <EXEC_ID>` and `fusion trace events <EXEC_ID>` CLI subcommands**
- [ ] **Step 2: Add integration test suite in `tests/runtime_events_tests.rs`**
- [ ] **Step 3: Run full workspace quality checks**

Run:
1. `cargo test --lib events`
2. `cargo test --test runtime_events_tests`
3. `cargo test --bin fusion`
4. `cargo clippy --all-targets -- -D warnings`

---

## Verification Plan

### Automated Tests
- `cargo test events::bus`
- `cargo test events::projection`
- `cargo test events::consumers::timeline`
- `cargo test events::consumers::checkpoint`
- `cargo test events::consumers::storage`
- `cargo test --test runtime_events_tests`
- `cargo test --bin fusion`
- `cargo clippy --all-targets -- -D warnings`

### CLI Command Execution
Command: `cargo run --bin fusion -- trace timeline exec-demo`
Command: `cargo run --bin fusion -- trace events exec-demo`
Expected output: Displays millisecond-accurate timeline rendering and raw JSON event envelope stream.
