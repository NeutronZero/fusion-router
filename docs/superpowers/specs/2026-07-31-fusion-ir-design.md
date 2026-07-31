# fusion-ir Crate Design

**Status:** Approved (pending user review of this document)
**Milestone:** v0.13.1 — Compiler Core, Task 1 (Workflow IR data model + builder)
**Date:** 2026-07-31
**Relates to:** ADR-032, ADR-033, `docs/specifications/architecture-v0.13.md`, frozen contract 2 (`src/ir/workflow.rs`, migrated verbatim)

## 1. Goals

- Establish `crates/fusion-ir` as the canonical, provider-independent definition of Workflow IR, per the frozen v0.13.0 architecture.
- Make Workflow IR a first-class crate — the architectural center of the compiler. Dependency rule: **everything else in the FusionRouter stack depends on `fusion-ir`; `fusion-ir` depends on nothing in the stack** (only general-purpose libraries: `serde`, `serde_json`, `uuid`, `thiserror`).
- Deliver an immutable `WorkflowIR` that is the single canonical graph representation, with deterministic canonical serialization from day one.
- Turn the frozen architectural laws into executable checks: structural, semantic, and architectural validation layers plus named conformance tests.
- Preserve public API compatibility: `fusion_router::ir` becomes a thin re-export shim.

## 2. Non-goals

- No execution-plan, scheduler-graph, provider-graph, or runtime-graph concepts in the crate. Workflow IR is the immutable, provider-independent description of logical work — nothing more.
- No compiler passes, runtime, providers, or scheduling code inside the crate.
- No `WorkflowGraph` type in v1 (see Section 5).
- No capability-compatibility semantic checks in v1 (deferred until the Capability Registry is executable; a documented hook is reserved).
- No `canonical_hash()` (SHA256) in v0.13.1 (noted as a future extension in Section 12).
- No planner/optimizer logic in the crate. The `intent_to_workflow()` lowering lives in the main crate and is explicitly the **Planner's initial lowering** (see Section 11).

## 3. Crate layout

```
crates/fusion-ir/
├── lib.rs         — crate docs, public API re-exports, canonical-graph invariant statement
├── workflow.rs    — WorkflowIR, WorkflowMetadata
├── node.rs        — WorkflowNode, WorkflowNodeKind (9 frozen kinds)
├── edge.rs        — WorkflowEdge, WorkflowEdgeKind (6 frozen kinds)
├── builder.rs     — WorkflowBuilder
├── validate.rs    — ValidationError, ValidationReport, validation layers
├── version.rs     — WORKFLOW_IR_VERSION
├── serialize.rs   — crate-private deterministic canonical JSON
└── error.rs       — WorkflowIrError (serde/validation error wrapper)
```

Workspace integration:

- Add `crates/fusion-ir` to `[workspace].members` in the root `Cargo.toml`.
- Add `fusion-ir = { path = "crates/fusion-ir" }` to the main crate's `[dependencies]`.
- Replace `src/ir/mod.rs` with the compatibility shim: `pub use fusion_ir::*;`
- Delete `src/ir/workflow.rs` (types moved verbatim into the crate).

## 4. Public API

Minimal surface; frozen type names kept verbatim (architecture documents outrank stylistic preferences):

```rust
pub use builder::WorkflowBuilder;
pub use edge::{WorkflowEdge, WorkflowEdgeKind};
pub use node::{WorkflowNode, WorkflowNodeKind};
pub use validate::{ValidationError, ValidationReport};
pub use version::WORKFLOW_IR_VERSION;
pub use workflow::{WorkflowIR, WorkflowMetadata};
```

`serialize.rs`, validation internals, and helper utilities remain crate-private unless a consumer demonstrates a need.

## 5. WorkflowIR model

Frozen contract types, moved verbatim:

```rust
pub struct WorkflowIR {
    pub version: u16,
    pub workflow_id: Uuid,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
    pub metadata: WorkflowMetadata,
}

pub struct WorkflowNode {
    pub id: String,
    pub kind: WorkflowNodeKind,          // Task, Tool, Retrieval, Memory, Review,
                                         // Judge, Security, Aggregation, Output
    pub capability: Option<String>,
    pub config: BTreeMap<String, serde_json::Value>,
}

pub struct WorkflowEdge {
    pub from: String,
    pub to: String,
    pub kind: WorkflowEdgeKind,          // Sequential, Parallel, Conditional, Retry, Merge, Loop
    pub condition: Option<String>,
}
```

All structs keep `#[serde(deny_unknown_fields)]`. All types derive `Debug, Clone, Serialize, Deserialize`.

**Deviations from the frozen module (both deliberate):**

1. `config` uses `BTreeMap` instead of `HashMap`. This is not a convenience change: it is **required to satisfy the frozen architectural invariant of deterministic serialization**. A `HashMap` does not guarantee deterministic iteration order. The canonical representation uses `BTreeMap` to guarantee deterministic serialization; this is an implementation refinement that preserves the WorkflowIR contract (identical JSON format, sorted keys).
2. Types move from `src/ir/workflow.rs` into the crate; the `fusion_router::ir` path is preserved by the re-export shim.
3. Struct fields are `pub(crate)`, not `pub`, with read-only getters (`version()`, `workflow_id()`, `nodes()`, `edges()`, `metadata()`, `id()`, `kind()`, `capability()`, `config()`, `from()`, `to()`, `condition()`). This is the compile-time enforcement of the immutability law below: the frozen code shape made literal construction and mutation possible from outside the crate, which no Rust visibility setting can prevent while keeping fields `pub`. Fields stay `pub(crate)` so the crate itself (builder, validation, serialization) keeps the frozen field layout; the JSON/serde contract, type names, and derives are unchanged.

**Canonical-graph invariant (documented in `lib.rs` crate docs):**

> WorkflowIR is the canonical immutable graph representation. There is intentionally no separate WorkflowGraph type. All graph operations are performed directly on WorkflowIR. Additional graph views may be introduced in future versions without changing the WorkflowIR contract.

**Metadata extensibility:** `WorkflowMetadata` is intentionally extensible while remaining provider-independent — future additions (telemetry hints, provenance, planner annotations) must not require changes to the core graph model.

**Immutability:** public construction happens only through `WorkflowBuilder`. `WorkflowIR`/`WorkflowNode`/`WorkflowEdge` constructors are crate-private; no mutation methods are exposed. Every publicly constructed `WorkflowIR` has passed structural validation.

## 6. Builder

`WorkflowBuilder → WorkflowIR`, validating as it goes (fail-fast):

```rust
WorkflowBuilder::new()
    .task("n1", "CodeGeneration")          // (id, capability)
    .task_with_config("n3", "Search", config)
    .output("n2")
    .sequential("n1", "n3")
    .merge("n3", "n2")
    .metadata(WorkflowMetadata { .. })
    .build()                               // -> Result<WorkflowIR, ValidationError>
```

- Node methods mirror the 9 frozen kinds: `task`, `tool`, `retrieval`, `memory`, `review`, `judge`, `security`, `aggregation`, `output` — each taking `id` plus optional capability/config.
- Edge methods mirror the 6 frozen kinds: `sequential`, `parallel`, `conditional`, `retry`, `merge`, `loop` — each taking `(from, to)` plus optional condition.
- Duplicate node ID → `ValidationError` at that call. Edge referencing a missing node → `ValidationError` at that call.
- `workflow_id` defaults to a fresh `Uuid`; `with_workflow_id(uuid)` overrides (required for deterministic replay tests).
- `build()` runs full structural validation and returns the final `WorkflowIR`.
- The builder itself is the only public construction path — no public raw constructors.

## 7. Validation

Three layers, separated by responsibility:

### Structural (pure graph validity — no meaning)

- Unique node IDs.
- Every edge endpoint references an existing node (no dangling edges).
- At least one root node exists (no incoming edges).
- Every node is reachable from some root (no unreachable nodes).

### Semantic (meaning of edge/node kinds)

- Only `Loop` edges may create cycles. `Sequential`, `Parallel`, `Conditional`, `Retry`, and `Merge` edges may not participate in a cycle. This rule lives here because it is about the meaning of edge kinds, not graph well-formedness.
- `Conditional` edges require a `condition` expression.
- `Retry` edges require a retryable source node kind (`Task`, `Tool`, `Retrieval`).
- `Merge` edges require at least two incoming edges into the merge target.
- `Output` nodes may not have outgoing edges.
- Capability-compatibility checks: **deferred** to the milestone where the Capability Registry becomes executable. The validation API reserves a documented hook for this layer.

### Architectural (executable frozen laws)

- **Provider-free:** reject provider-identifying configuration fields reserved by the architecture (initially `model`, `provider`, and `endpoint`), including a recursive scan of `config` maps. The reserved list may be expanded later without redefining the law.
- **Versioned:** `version == WORKFLOW_IR_VERSION`, enforced at construction and deserialization.
- **Deterministic:** canonical serialization is stable (Section 8).
- **Immutable:** no mutation surface after construction.
- **Replayable:** IR → JSON → IR round-trip is lossless, including `workflow_id`.

### Result types

- `ValidationError` (thiserror enum): builder path, first error, fail-fast.
- `ValidationReport`: full analysis path (`WorkflowIR::validate()`), collects all issues with node/edge references, in **deterministic order** (reports are sorted by node/edge id so snapshots and CI are stable).

## 8. Serialization

Deterministic canonical JSON, first-class from day one:

- `to_canonical_json(&self) -> String` and `from_json(&str) -> Result<WorkflowIR, WorkflowIrError>`.
- **Canonical ordering** (explicit): nodes serialized sorted by `id`; edges serialized sorted by `(from, to, kind, condition)`; `config` via `BTreeMap` (sorted keys); serde struct field order is stable. *(Deviation note: the frozen wording was `(from, to, kind)`, which is not a total order — two conditional edges between the same nodes tie and stable sort then leaks insertion order, violating byte-determinism. `condition` completes the edge identity; exact duplicates are byte-identical elements whose relative order cannot affect output.)*
- **Determinism is a property of the IR, not the builder:** the builder accepts any insertion order; canonical serialization establishes the canonical ordering.
- Canonical invariant: identical logical workflows — even built via different construction orderings — produce byte-identical output.
- The canonical form is the basis for future fingerprints, caching, snapshots, and telemetry correlation.

## 9. Versioning

- `WORKFLOW_IR_VERSION: u16 = 1`, defined in `version.rs` and re-exported.
- `WorkflowIR.version` is enforced by architectural validation: `version != WORKFLOW_IR_VERSION` → error.
- `deny_unknown_fields` rejects payloads with unknown fields — silent acceptance of newer schemas would corrupt replayability.
- Migration path: future version bumps add a `migrate(previous_version, ir)` step; old payloads must be migrated explicitly, never silently accepted.

## 10. Testing

### Module tests

- Builder fail-fast: duplicate IDs, dangling edge refs, missing condition, illegal cycle.
- Semantic legality: `Conditional` requires condition; `Retry` source must be retryable; `Output` has no outgoing edges; `Merge` needs ≥ 2 incoming; only `Loop` edges may cycle.
- Version enforcement and `deny_unknown_fields` behavior.
- Serialization round-trip, config key ordering, canonical determinism.

### Executable architecture-law tests (named conformance tests)

- `workflow_ir_is_deterministic` — same logical workflow built via different construction orderings → byte-identical canonical JSON.
- `workflow_ir_round_trip_is_lossless` — IR → JSON → IR yields identical nodes, edges, metadata (including f64s).
- `workflow_id_stable_across_round_trip` — `workflow_id` identical across the round trip.
- `workflow_ir_contains_no_provider_information` — recursive scan finds no `model`/`provider`/`endpoint` keys.
- `workflow_ir_is_immutable` — only the builder constructs; no public mutation surface.
- `workflow_ir_validation_is_deterministic` — `validate()` reports issues in deterministic order regardless of input ordering, so snapshot tests and CI are stable.
- `runtime_never_mutates_workflow_ir` — reserved; enforced in the runtime milestone (v0.13.2) against this crate's immutable surface.

## 11. Integration with NormalizedIntent (Planner's initial lowering)

Per the frozen pipeline `NormalizedIntent → Planning → WorkflowIR → Compiler → Execution ABI`, the lowering is the **first implementation of the Planner** — the actual compiler begins *after* WorkflowIR exists. It therefore lives in the main crate, not in `fusion-ir` (avoiding a circular dependency: `fusion-router` depends on `fusion-ir`).

- New file `src/intent/lowering.rs` in the main crate: `pub fn intent_to_workflow(&NormalizedIntent) -> Result<WorkflowIR, ValidationError>`.
- v1 mapping (deliberately narrow): a single primary task node (capability from the intent, reasoning-budget-derived `config`), one `Output` node, one `Sequential` edge; `Constraints`/`Budget` map into `WorkflowMetadata` (`estimated_cost`, `estimated_tokens`).
- Integration test: `NormalizedIntent` → `WorkflowIR` through `fusion_router::ir` (proves the shim and the end-to-end type flow).
- Explicitly out of scope for this milestone: multi-node lowering, review/judge insertion, retrieval planning, optimization. Those are later compiler-frontend work.

## 12. Future extensions

- `WorkflowGraph` as an *internal* view over `WorkflowIR` if graph algorithms (scheduling, optimization) ever demand it — never as a parallel public representation.
- `canonical_hash()` implemented as `SHA256(canonical_json)` for execution fingerprints, telemetry correlation, cache keys, distributed execution, and replay.
- Capability-compatibility semantic validation once the Capability Registry is executable (hook reserved in Section 7).
- Richer planner lowering: multi-node workflows, review/judge insertion, retrieval planning.
- Version-migration tooling for future `WORKFLOW_IR_VERSION` bumps.
