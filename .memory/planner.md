# FusionRouter Planner

## Overview

The planner converts a user intent and `ContextSnapshot` into a `WorkflowIR` — a high-level abstract plan describing what needs to happen without specifying how.

**Location:** `src/planner/`
**Key types:** `Planner` trait, `IntentPlanner`, `SimplePlanner`, `DynamicPlanner`, `WorkflowPlanner`

## Planner Implementations

### SimplePlanner (`src/planner/simple.rs`)
- Default fallback planner
- Produces a single-node `WorkflowIR` for simple requests
- Used when no workflow definition matches

### WorkflowPlanner (`src/planner/workflow.rs`)
- Registry-first planner (ADR-014)
- Looks up matching `WorkflowDefinition` in `WorkflowRegistry` (`src/workflow/`)
- Falls back to `SimplePlanner` on no match
- Supports template instantiation: parameters from requirements → `WorkflowIR`

### DynamicPlanner (`src/planner/dynamic_planner.rs`)
- Uses an LLM to generate `WorkflowIR` from intent (ADR-015)
- Three modes: `Static` (no LLM), `Dynamic` (LLM generates), `Hybrid` (LLM refines registry match)
- Safety guards: max node count, timeout, max iterations

### IntentPlanner (`src/planner/intent_planner.rs`)
- Entry point planner that delegates to the appropriate sub-planner
- Based on intent classification from `RequirementsExtractor`

## Capability Resolver (`src/planner/resolver/capability/`)

| Component | File | Purpose |
|-----------|------|---------|
| `CapabilityResolver` | `resolver.rs` | Resolves abstract capability references to concrete instances |
| `CapabilityGraph` | `graph.rs` | Dependency DAG for capabilities |
| Capability Planner Cache | `mod.rs` | LRU cache for resolved capability plans |

The resolver is called during the **Capability Resolution** compiler pass to bind abstract capability references to concrete `CapabilityInstance` objects.

Policy enforcement (v0.13.1, H13/ADR-034): `apply_policy` runs on all resolution paths — required, version-constrained, optional, transitive dependencies inside `expand_dependencies`, and a final re-verification over the resolved instance set. Any deny-list hit (or allow-list miss) fails resolution with `ResolverError::PolicyDenied`; no capability can bypass policy through version constraints, optional requirements, aliases, or transitive deps.

## Workflow Registry (`src/workflow/`)

| Component | File | Purpose |
|-----------|------|---------|
| `WorkflowRegistry` | `registry.rs` | Loads and caches YAML workflow definitions |
| `WorkflowDefinition` | `mod.rs` | YAML schema for workflow templates |

Workflows are defined in YAML and matched against requirements during planning. Template parameters allow dynamic instantiation.

## WorkflowIR

The output of planning — a high-level IR with semantic node types:

| Node Type | Purpose |
|-----------|---------|
| `Generate` | LLM generation |
| `Review` | LLM review/critique |
| `Judge` | LLM evaluation |
| `Transform` | Data transformation |
| `Gate` | Policy gate check |
| `Conditional` | Conditional branch |
| `Loop` | Iteration |
| `Split` | Parallel fan-out |
| `Join` | Synchronization barrier |
| `Barrier` | General synchronization |

**Design doc:** `docs/specifications/workflow-ir.md`

## Key Invariants

- Planner produces `WorkflowIR`, never `ExecutionGraph` (that's the compiler's job)
- Planner may call LLMs (DynamicPlanner) — compiler never does
- Policy-driven planning: `PlannerPolicy` influences which strategies/approaches are selected
- Evidence-informed: prior execution feedback may influence planning

## Related ADRs

- ADR-002: Planner produces WorkflowIR, policy-driven, evidence-informed
- ADR-013: WorkflowRegistry, WorkflowDefinition YAML schema
- ADR-014: WorkflowPlanner (registry-first, fallback)
- ADR-015: DynamicPlanner (LLM-generated WorkflowIR)
- ADR-023: Capability resolution late-binding
- ADR-025: Connector abstraction (planner agnostic to connectors)
