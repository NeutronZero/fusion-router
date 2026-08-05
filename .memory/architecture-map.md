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
  ├── build_compiler ─── Sole production construction path (ADR-034)
  │     src/compiler/mod.rs
  │
  ├── 1. ConstraintValidation ─── Validates WorkflowIR structure (non-empty)
  │     src/compiler/passes/legacy_passes.rs
  │
  ├── 2. ControlFlowValidation ─── Validates edges, control-flow node shapes, acyclicity
  │     src/compiler/passes/legacy_passes.rs
  │
  ├── 3. ModelResolution ─── Binds models from ModelCatalog when unspecified
  │     src/compiler/passes/legacy_passes.rs
  │
  ├── 4. BudgetOptimisation ─── Applies resource budgets via ResourceManager
  │     src/compiler/passes/legacy_passes.rs
  │
  ├── 5. PolicyCompilerPass (optional) ─── Appended when PolicyIR supplied; deny = compile error
  │     src/compiler/passes/policy.rs
  │
  └── lower_to_graph ─── Direct structural lowering WorkflowIR → ExecutionGraph
        src/compiler/mod.rs
      │
      ▼
┌──────────────────────────────────────────────────┐
│  ExecutionGraph                                   │
│  src/types/mod.rs                                │
│  10 node kinds: LLMGenerate, LLMReview, LLMJudge,│
│  Transform, Gate, Conditional, Loop, Split,      │
│  Join, Barrier                                   │
│  (strategy is a field on every node)             │
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

## v0.13 Contract Bridges (frozen contracts ↔ live v0.12 path)

```
NormalizedIntent → WorkflowIR (fusion_ir)  src/intent/lowering.rs
      ▼  ir::adapter::workflow_to_types     src/ir/adapter.rs   (deterministic id hashing)
types::WorkflowIR → build_compiler → ExecutionGraph   (live path)
      ▼  abi::from_graph::abi_from_graph   src/abi/from_graph.rs  (ABI generator, contract 3)
ExecutionAbi → abi::to_graph::graph_from_abi         src/abi/to_graph.rs  (runtime binds providers)
      ▼  eri::local_runtime::LocalEri      src/eri/local_runtime.rs  (contract 5 over scheduler/executor)
Verified end-to-end by tests/contract_wiring.rs
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
  (DynamicPlanner)    capabilities           4 mandatory passes + optional policy pass
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
Cargo.toml ─── workspace root (fusion-router v0.13.0)
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
