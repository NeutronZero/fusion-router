# v0.11 — Runtime Intelligence: Execution Event Model Design Specification

> **Goal:** Transition FusionRouter into an event-driven runtime by establishing a canonical, strongly-typed, event-sourced `ExecutionEvent` stream, abstract `EventBus` infrastructure, and `EventProjection` framework that powers OpenTelemetry tracing, timeline visualization, node-level checkpointing, persistent state storage, and future Execution Memory.

---

## 1. Principles & Architectural Constitution

1. **Event Stream as Primary Runtime ABI:**
   All runtime components (Scheduler, Executor, ProviderRouter, ToolRegistry, ResourceManager) emit append-only, strongly-typed `ExecutionEvent` variants. Observability, storage, and recovery consume events via projections rather than direct instrumentation.
2. **Schema Versioning & Correlation Identity:**
   Every event envelope carries `schema_version`, `event_id`, `workflow_id`, `execution_id`, `correlation_id` (for logical sub-operations), `sequence_number`, `timestamp`, `parent_event_id`, and typed payload.
3. **Interface-Driven Bus Abstraction:**
   An abstract `EventBus` trait (`publish`, `subscribe`) decouples event propagation from transport. `BroadcastEventBus` (backed by `tokio::sync::broadcast`) serves as the initial implementation.
4. **Decoupled Projection Framework:**
   Projections implement the `EventProjection` trait (`handle_event()`). Projections operate asynchronously and isolated from the core execution loop.
5. **Policy-Driven Recovery:**
   `CheckpointEngine` triggers checkpoints via a configurable `CheckpointPolicy` (`EveryNode`, `EveryNthNode`, `Timed`, `Manual`).

---

## 2. Subsystem Architecture & Pipeline

```text
Planner / Compiler / Scheduler / Executor / ResourceManager
                            │
                            ▼ (emits)
              EventBus Trait (BroadcastEventBus)
                            │
                            ▼
                    Projection Framework
                            │
  ┌──────────────┬──────────┴───┬──────────────┬──────────────┐
  ▼              ▼              ▼              ▼              ▼
1. OTel      2. Timeline   3. Checkpoint  4. Persistent   5. Memory
Exporter     Visualizer      Engine       Event Store     Bridge
```

---

## 3. Data Models & Event Taxonomy

### 3.1 Canonical Event Envelope (`src/events/mod.rs`)

```rust
pub const EVENT_SCHEMA_VERSION: &str = "fusion.router.event.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEventEnvelope {
    pub schema_version: String,            // "fusion.router.event.v1"
    pub event_id: String,                  // e.g. "evt-20260727-0042"
    pub workflow_id: String,               // e.g. "wf-code-review-8a9f"
    pub execution_id: String,              // e.g. "exec-7b3c"
    pub correlation_id: Option<String>,    // e.g. "corr-llm-call-99"
    pub sequence_number: u64,              // Monotonic 1-based index
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub parent_event_id: Option<String>,
    pub payload: ExecutionEvent,
}
```

### 3.2 Strongly-Typed Event Taxonomy (`src/events/payload.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ExecutionEvent {
    // Workflow Lifecycle
    WorkflowStarted { intent: String, input_tokens: usize },
    WorkflowCompleted { total_duration_ms: u64, total_cost_usd: f64 },
    WorkflowFailed { error: String, failed_node_id: Option<String> },

    // Compilation & Scheduling
    WorkflowCompiled { node_count: usize, edge_count: usize, primitive_graph_hash: u64 },
    NodeScheduled { node_id: String, node_kind: String, dependencies: Vec<String> },

    // Node Execution Loop
    NodeStarted { node_id: String, target_model: Option<String> },
    NodeFinished { node_id: String, duration_ms: u64, prompt_tokens: u32, completion_tokens: u32 },
    NodeFailed { node_id: String, error: String, attempt: u32 },

    // Resilience & Retry
    RetryStarted { node_id: String, attempt: u32, backoff_ms: u64 },
    RetrySucceeded { node_id: String, attempt: u32 },

    // Transport, Provider & Tool Activity
    ProviderCalled { provider: String, model: String, prompt_bytes: usize },
    ProviderResponded { provider: String, model: String, duration_ms: u64, cost_usd: f64 },
    ToolInvoked { tool_name: String, node_id: String },
    ToolCompleted { tool_name: String, node_id: String, duration_ms: u64, success: bool },

    // Context & Resource Lifecycle
    ContextMaterialized { node_id: String, context_size_bytes: usize },
    ResourceAllocated { resource_type: String, amount: f64 },
    ResourceReleased { resource_type: String, amount: f64 },
    SemaphoreAcquired { resource_name: String, permits: u32 },
    SemaphoreReleased { resource_name: String, permits: u32 },
    BudgetExceeded { resource_type: String, limit: f64, actual: f64 },
}
```

---

## 4. Event Bus & Projection Framework Contracts

### 4.1 EventBus Trait (`src/events/bus.rs`)

```rust
#[async_trait::async_trait]
pub trait EventBus: Send + Sync {
    async fn publish(&self, envelope: ExecutionEventEnvelope) -> Result<(), GateError>;
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ExecutionEventEnvelope>;
}
```

### 4.2 Projection Framework (`src/events/projection.rs`)

```rust
#[async_trait::async_trait]
pub trait EventProjection: Send + Sync {
    fn name(&self) -> &'static str;
    async fn handle_event(&mut self, envelope: &ExecutionEventEnvelope) -> Result<(), GateError>;
}
```

---

## 5. Checkpoint Policy & Projections

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CheckpointPolicy {
    EveryNode,
    EveryNthNode(usize),
    Timed(std::time::Duration),
    Manual,
}
```

---

## 6. Revised Sprint Milestones (v0.11)

1. **Sprint N1:** Canonical `ExecutionEvent` & `EventBus` Trait (`src/events/bus.rs`).
2. **Sprint N1.5:** `EventProjection` Framework & Dispatcher (`src/events/projection.rs`).
3. **Sprint N2:** OpenTelemetry Tracing Projection (`src/events/consumers/otel.rs`).
4. **Sprint N3:** Runtime Timeline & Visualizer (`src/events/consumers/timeline.rs`).
5. **Sprint N4:** Policy-Driven Checkpoint Engine (`src/events/consumers/checkpoint.rs`).
6. **Sprint N5:** Persistent Event Store & CLI Inspector (`src/events/consumers/storage.rs`).
