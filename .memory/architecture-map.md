# Architecture Map

> One-page system map. Read this first in every session.

## Pipeline

```
Client Request
      │
      ▼
Axum HTTP Server ─── Middleware: Auth → CORS → Rate Limit → Request ID
  src/server/
      │
      ▼
ContextAssembler ─── Builds ContextSnapshot from system prompt, history, tools
  src/context/
      │
      ▼
RequirementsExtractor ─── Classifies intent, extracts complexity
  src/requirements/
      │
      ▼
Planner ─── Produces WorkflowIR
  src/planner/
  ├── WorkflowPlanner (registry-first, fallback to SimplePlanner)
  │     src/planner/workflow.rs
  ├── SimplePlanner (single node)
  │     src/planner/simple.rs
  ├── DynamicPlanner (LLM-generated, with safety guards)
  │     src/planner/dynamic_planner.rs
  └── CapabilityResolver
        src/planner/resolver/capability/
      │
      ▼
┌──────────────────────────────────────────────────┐
│  WorkflowIR                                       │
│  src/types/execution.rs                          │
│  Nodes: Generate, Review, Judge, Transform,      │
│         Gate, Conditional, Loop, Split,          │
│         Join, Barrier                            │
│  docs/specifications/workflow-ir.md              │
└──────────────────────────────────────────────────┘
      │
      ▼
Compiler ─── Pure deterministic passes, no LLM calls
  src/compiler/
  │
  ├── 1. ConstraintValidation ─── Validates WorkflowIR structure
  │     src/compiler/passes/legacy_passes.rs
  │
  ├── 2. CapabilityResolution ─── Binds abstract capability → concrete instance
  │     src/planner/resolver/capability/
  │
  ├── 3. BudgetOptimisation ─── Applies resource budgets from policy
  │     src/compiler/passes/legacy_passes.rs
  │
  ├── 4. NodeFusion ─── Merges compatible sequential nodes
  │     src/compiler/passes/legacy_passes.rs
  │
  ├── 5. RetryFallbackInsertion ─── Wraps nodes with retry/fallback
  │     src/compiler/passes/legacy_passes.rs
  │
  ├── 6. SchedulingHints ─── Annotates nodes with scheduling metadata
  │     src/compiler/passes/legacy_passes.rs
  │
  └── 7. GraphVerification ─── Validates final ExecutionGraph
        src/compiler/passes/legacy_passes.rs
      │
      ▼
┌──────────────────────────────────────────────────┐
│  ExecutionGraph                                   │
│  src/types/execution.rs                          │
│  12 node kinds: LLMRequest, Strategy, ToolCall,  │
│  Connector, Conditional, Loop, Split, Join,      │
│  Barrier, Transform, Gate, Subgraph              │
│  docs/specifications/execution-graph.md          │
│  docs/specifications/compiler-passes.md          │
└──────────────────────────────────────────────────┘
      │
      ▼
Scheduler ─── Topology-driven DAG execution
  src/scheduler/
  ├── DefaultScheduler (local)
  │     src/scheduler/default.rs
  ├── DistributedScheduler (remote workers)
  │     src/scheduler/distributed.rs
  └── WorkQueue (topological queue)
        src/scheduler/work_queue.rs
      │
      ▼
Executor ─── Dispatches nodes to handlers
  src/executor/
  ├── CapabilityExecutor (unified dispatch)
  │     src/executor/capability_executor.rs
  ├── Providers (LLM calls)
  │     src/providers/
  ├── Strategies (multi-step reasoning)
  │     src/strategies/
  ├── Tools (built-in tools)
  │     src/tools/
  └── Connectors (external services)
        src/connectors/
      │
      ▼
┌──────────────────────────────────────────────────┐
│  ExecutionResult                                  │
│  crates/fusion-plugin-api/src/lib.rs             │
└──────────────────────────────────────────────────┘
```

## Supporting Subsystems

```
Session & Lifecycle           Events & Telemetry
  src/session/                  src/events/
  src/lifecycle/                src/telemetry/

Policy & Governance          Capability System
  src/policy/                   src/capability/
  src/release/                  src/planner/resolver/capability/

Resource Management          Plugin System
  src/resource/                 src/plugin/
                                src/wasm/ (feature-gated)
Configuration                Caching
  src/config/                   src/cache/ (feature-gated)

Security                     Developer Experience
  src/middleware/                src/devex/
  src/feature_gate/

Triggers                      Connectors
  src/trigger/                   src/connectors/
```

## External SDK Crates

```
fusion-plugin-api (crates/fusion-plugin-api/)
  ├── Plugin, CapabilityPlugin, CapabilityExecutor traits
  ├── CapabilityContract, CapabilityInstance, CapabilityId
  ├── ExecutionResult, ExecutionError
  └── Permission (5 variants)

fusion-capability-macros (crates/fusion-capability-macros/)
  └── #[capability] attribute macro

fusion-capability-sdk (crates/fusion-capability-sdk/)
  ├── CapabilityBuilder (builder pattern)
  ├── CapabilityManifestBuilder
  └── SchemaBuilder (JSON Schema)
```

## Architectural Layer Rules

```
Planner ───────────→ WorkflowIR ──────────→ Compiler
  May call LLMs       May reference          Pure, deterministic
  (DynamicPlanner)    capabilities           7 passes
                       No provider refs       No LLM calls

Compiler ──────────→ ExecutionGraph ──────→ Scheduler
  Freezes graph                              Reads frozen graph
  Owns graph lifecycle                       Topology-driven
                                              Never mutates graph

Scheduler ─────────→ Executor ────────────→ Provider
  Output selection    Dispatches via          LLM abstraction
  State machine       CapabilityExecutor      Circuit breaker
                      Resource guards

Dependency direction: left → right only
No layer reaches backward
```

## File System Layout

```
Cargo.toml ─── workspace root (fusion-router v0.12.0)
src/ ───────── 125+ files across 33 modules
crates/ ────── 3 SDK crates
plugins/ ───── example plugins
docs/ ──────── architecture, ADRs, specifications, decisions
benches/ ───── benchmarks
tests/ ─────── integration tests
examples/ ──── usage examples
workflows/ ─── YAML workflow definitions
.memory/ ───── this handbook
```
