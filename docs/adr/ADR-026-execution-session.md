# ADR-026: Execution Session & Storage Engine Decoupling

- **Status**: Proposed
- **Date**: July 2026
- **Context**: FusionRouter v0.10.0 Stateful Session Runtime
- **Deciders**: FusionRouter Core Architecture Team

---

## Context

Short-lived request-response models cannot support long-running autonomous workflows that span human approvals, async webhooks, or scheduled triggers. Stateful execution requires session tracking, checkpointing, pause/resume, and persistence.

---

## Decisions

### 1. Decoupling `ExecutionSession` from `SessionStore`

We separate the runtime session state controller (`ExecutionSession`) from the persistence storage backend (`SessionStore`).

```rust
pub struct ExecutionSession {
    pub session_id: SessionId,
    pub graph_hash: GraphHash,
    pub status: SessionStatus, // Running, Paused, Completed, Failed, Cancelled
    pub current_node_id: NodeId,
    pub state_vars: HashMap<String, serde_json::Value>,
}

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn save_checkpoint(&self, checkpoint: &SessionCheckpoint) -> Result<()>;
    async fn load_checkpoint(&self, session_id: &SessionId) -> Result<Option<SessionCheckpoint>>;
    async fn update_status(&self, session_id: &SessionId, status: SessionStatus) -> Result<()>;
}
```

### 2. Supported `SessionStore` Implementations

- `MemorySessionStore`: In-memory hashmap store for testing and fast ephemeral workflows.
- `SqliteSessionStore`: Single-file persistent storage for desktop agents and edge deployments.
- `PostgresSessionStore`: Production enterprise storage for distributed clusters.
- `RedisSessionStore`: Distributed caching store for high-throughput session state.

---

## Consequences

- Workflows can be safely paused waiting for human approval (`ApprovalNode`) and resumed hours or days later without losing context.
- Storage layer can be changed via configuration without altering session runtime code.
