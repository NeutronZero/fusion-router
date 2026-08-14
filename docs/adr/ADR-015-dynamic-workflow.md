# ADR-015: Dynamic Workflow Generation

## Status
Accepted

## Context
FusionRouter currently supports two planning modes:
1. **YAML‑based workflows** – workflow definitions loaded from `workflows/*.yaml`, matched by intent/complexity.
2. **SimplePlanner fallback** – a single‑node generate plan when no YAML definition matches.

Both are static: the execution graph is fully determined at configuration time. Users cannot adapt the workflow to novel or ambiguous requests without writing new YAML files.

We need a mode where an LLM generates the `WorkflowIR` dynamically from the user's prompt, validated through existing compiler passes.

## Decision
Introduce a `DynamicPlanner` that implements the `Planner` trait by:

1. **Constructing a planning prompt** from `Requirements` and `ContextSnapshot` (intent, complexity, constraints).
2. **Calling a configured model** to produce a `WorkflowIR` in JSON format.
3. **Validating the generated IR** through existing validation logic.
4. **Falling back** to `SimplePlanner` if validation fails or safety limits are exceeded.

The `WorkflowPlanner` gains a `mode` configuration (`"static"`, `"dynamic"`, `"hybrid"`) that controls delegation:
- **static**: current behaviour – registry, then SimplePlanner.
- **dynamic**: DynamicPlanner only (no YAML registry matching).
- **hybrid**: try DynamicPlanner first, fall back to registry, then SimplePlanner.

## Consequences

### Positive
- Users can handle novel requests without predefined YAML workflows.
- Generated plans are validated by the same compiler passes – no new failure modes.
- Safety limits guard against runaway generation.

### Negative
- Adds a model call in the planning phase (latency + cost).
- Generated plans may be inconsistent or low‑quality – mitigated by validation and fallback.
- Requires a capable model to produce valid JSON `WorkflowIR`.

## Safety Guards
- `max_generated_nodes` (default: 20) – caps graph size.
- `generation_timeout` (default: 10s) – caps model response time.
- `max_iterations` (default: 10) – caps loop iteration count in generated graphs.
- Generated IR is validated by `ConstraintValidationPass` and `ControlFlowValidationPass` before acceptance.

## Configuration
```yaml
planner:
  mode: "hybrid"           # static | dynamic | hybrid
  dynamic:
    model: "zen-7b"
    max_generated_nodes: 20
    generation_timeout_ms: 10000
    max_iterations: 10
```
