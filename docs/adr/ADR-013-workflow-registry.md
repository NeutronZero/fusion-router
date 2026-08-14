# ADR-013: Workflow Registry

## Status
Accepted

## Context
FusionRouter previously produced `WorkflowIR` exclusively via the `SimplePlanner`, which always generated a single-node linear graph. As the system grows, users and integrators need the ability to define reusable workflow templates — named multi-node DAGs with specific capability requirements — that can be matched to incoming requests and instantiated as `WorkflowIR` at plan time.

Key requirements:
- Named workflow definitions that are discoverable and selectable at runtime
- YAML-based authoring for ease of use (no Rust compilation required)
- Capability-based matching: workflows declare required intents, minimum complexity, and file dependencies
- Template-based node/edge definitions with placeholders for model, strategy, and config
- Directory-based loading for managing multiple workflow files

## Decision

### 1. `WorkflowDefinition` Schema

Each workflow definition is a self-describing template:

```yaml
name: code-review
description: Review code changes with analysis and feedback
required_intents: ["Code", "Debug"]
min_complexity: 0
requires_files: true
node_templates:
  - kind: Generate
    strategy: Single
    model: claude-sonnet-4-20250514
    config:
      system_prompt: "Review the following code for bugs, style issues, and improvements."
  - kind: Review
    strategy: Reflection
    config:
      review_focus: "correctness,style,performance"
edges:
  - from: 0
    to: 1
```

- `name` — unique identifier, used as the registry key
- `description` — human-readable summary
- `required_intents` — list of `Intent` variants the workflow can handle; empty means all
- `min_complexity` — minimum `ComplexityLevel` threshold (0=Low, 3=Critical); 0 means all
- `requires_files` — if true, the workflow is only selectable when files are present
- `node_templates` — ordered list of node definitions; each has kind, strategy, optional model, and config
- `edges` — directed edges between node template indices; each has from, to, and optional condition

### 2. `WorkflowRegistry` Structure

A thread-safe collection of `WorkflowDefinition` instances backed by a `HashMap<String, WorkflowDefinition>`:

- `register(def)` — inserts a definition keyed by name
- `get(name)` — retrieves a definition by name
- `list()` — returns all registered definitions
- `select(reqs)` — finds the first definition whose capability filters match the given `Requirements`
- `load_dir(path)` — scans a directory for `.yaml`/`.yml` files, deserializes each as `WorkflowDefinition`, and registers them

### 3. Capability Matching (`can_handle`)

A workflow matches a `Requirements` struct when all of the following hold:
- If `required_intents` is non-empty, the request's `intent_classification` must be in the list
- If `requires_files` is true, the request's `has_files` must be true
- The workflow's `min_complexity` maps to `ComplexityLevel` ordinal and must be <= the request's complexity level

No ranking or scoring is performed — the first matching definition is returned.

### 4. Instantiation

`WorkflowDefinition::instantiate(reqs)` produces a `WorkflowIR`:
- Each `NodeTemplate` becomes an `IRNode` with a fresh UUID, applying the template's kind, strategy, model, and config
- If no model is specified for LLM nodes (Generate, Review, Judge), a default model is assigned
- Each `EdgeTemplate` becomes an `IREdge` pointing from the `from`-index node to the `to`-index node
- `IRMetadata` is computed based on complexity level and node count

### 5. File Format

YAML files use `serde_yaml` for deserialization. The `WorkflowDefinition` struct derives `Serialize` and `Deserialize` for round-trip support. File discovery is flat (no recursive subdirectory scanning).

## Consequences

- Users define workflows without Rust compilation — a pure YAML authoring experience
- The registry is extensible at startup via `load_dir` and at runtime via `register`
- Capability matching is simple and deterministic (first-match, no scoring)
- Instantiation is pure and does not perform validation — the compiler pipeline handles that
- YAML deserialization errors are surfaced immediately at startup, not at request time
- Third-party plugin authors can ship workflow YAML files alongside their plugins
