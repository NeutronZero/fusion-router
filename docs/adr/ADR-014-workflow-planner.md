# ADR-014: Workflow Planner

## Status
Accepted

## Context
With the introduction of `WorkflowRegistry` (ADR-013), the system now has a store of named workflow definitions. The planner layer needs to integrate this registry into the existing planning pipeline while maintaining backward compatibility and a clean fallback path.

Previously, `SimplePlanner` always produced a single `Generate` node with no edges. The new `WorkflowPlanner` must:
- Check the registry for a matching workflow definition before falling back to the simple planner
- Support gradual adoption — existing request paths must continue working unchanged
- Keep the `Planner` trait interface stable so other planner implementations remain possible
- Handle the case where no registry is configured gracefully

## Decision

### 1. `WorkflowPlanner` Structure

A new planner implementation that wraps both a `WorkflowRegistry` reference and a `SimplePlanner` instance:

```rust
pub struct WorkflowPlanner {
    registry: Arc<WorkflowRegistry>,
    fallback: SimplePlanner,
}
```

The registry is shared via `Arc` so it can be used concurrently by the planner and the HTTP handler layer (for listing/debugging).

### 2. Selection Logic

When `WorkflowPlanner::plan()` is called:

1. **Query the registry**: `self.registry.select(requirements)` finds the first matching definition using capability-based matching (intent, complexity, files)
2. **If matched**: Call `definition.instantiate(requirements)` to produce a `WorkflowIR` directly from the template
3. **If not matched**: Delegate to `self.fallback.plan(requirements, policies, evidence)` which produces a single-node linear IR

This is a simplified decision tree:
```
plan(reqs) → registry.select(reqs)
  ├── Some(def) → def.instantiate(reqs) → WorkflowIR
  └── None → fallback.plan(reqs, policies, evidence) → WorkflowIR
```

### 3. Planner Trait Stability

The `Planner` trait remains unchanged:

```rust
#[async_trait]
pub trait Planner: Send + Sync {
    async fn plan(
        &self,
        requirements: &Requirements,
        policies: &[Policy],
        evidence: Option<&EvidenceSnapshot>,
    ) -> WorkflowIR;
}
```

The `WorkflowPlanner` ignores `policies` and `evidence` for matched workflows (the template defines the policy implicitly). For the fallback path, these parameters are forwarded to `SimplePlanner` unchanged.

### 4. AppState Integration

In `AppState::new()`:

```rust
let mut workflow_registry = WorkflowRegistry::new();
let _ = workflow_registry.load_dir("workflows");
let workflow_registry = Arc::new(workflow_registry);

let planner: Arc<dyn Planner + Send + Sync> = Arc::new(
    WorkflowPlanner::new(workflow_registry.clone()),
);
```

Key points:
- `workflow_dir` loading happens once at startup; failures are silently ignored (directory may not exist)
- The registry is shared between the planner and AppState for observability
- The planner field type changed from `Arc<SimplePlanner>` to `Arc<dyn Planner>` to support different implementations

### 5. Error Handling

- If `load_dir` fails (e.g., directory missing), the registry starts empty and all requests fall through to `SimplePlanner`
- YAML parse errors at startup are logged but do not prevent the server from starting
- No runtime errors from the planner — instantiation always succeeds

## Consequences

- All existing request paths continue working — unmatched intents fall through to `SimplePlanner`
- Workflow authors can define new templates without touching Rust code
- The planner is stateless beyond the shared registry reference
- The `dyn Planner` type allows future planners to be added without changing the handler interface
- Registry loading failures are non-fatal — the system degrades gracefully to the simple planner
