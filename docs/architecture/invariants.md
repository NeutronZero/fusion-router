# Architectural Invariants & Governance Constitution

This document defines the core invariants and architectural governance constitution for FusionRouter.

---

## Core Invariants

1. **All LLM interactions go through the Provider trait.** No subsystem calls an LLM API directly.
2. **The Compiler is a pipeline of pure passes.** Each pass takes an IR and returns a transformed IR or an error.
3. **The Planner produces a WorkflowIR, never an ExecutionGraph.** Compilation is a separate concern.
4. **Every ExecutionNode has exactly one Strategy.** The Strategy trait determines how it is expanded.
5. **Scheduling is topology-driven.** A node executes when all its dependency nodes have succeeded.
6. **The ResourceManager is the sole authority on budget.** No other component makes quota decisions.
7. **Telemetry is passive.** It observes and records but never alters execution.
8. **Evidence is derived from telemetry.** The EvidenceRepository aggregates raw records into snapshots.
9. **All config is external.** No hardcoded models, providers, or policies.
10. **Context is immutable once assembled.** No component modifies the ContextSnapshot after creation.
11. **Requirements are a heuristic, not a guarantee.** They guide but never constrain execution.
12. **Every public API is OpenAI-compatible.** `/v1/chat/completions` is the primary interface.
13. **Streaming is first-class.** All providers must support both streaming and non-streaming modes.
14. **Errors are typed.** Every fallible operation returns a structured error type, not a string.
15. **Capabilities are declarative.** The Planner operates on `CapabilityContract`, never on physical implementations.
16. **CapabilityRegistry is frozen at runtime.** `PluginManager` freezes the registry during startup; runtime lookups are read-only (`Arc<CapabilityRegistry>`).

---

## Compiler Phase Invariants (ADR-027 Matrix)

| Phase | May Do | Must Not Do |
|---|---|---|
| **Plugin Manager** | Discover plugins, validate manifests, run compatibility checks, freeze registry | Mutate registry after startup, execute workflow graphs |
| **Capability Resolver** | Resolve contracts, build `CapabilityGraph`, instantiate `CapabilityInstance` handles, query cache | Execute capability logic, rewrite workflow intent |
| **Planner** | Analyze user intent, extract requirements, construct abstract `PrimitiveGraph` IR | Bind concrete connectors, execute tools, evaluate security approvals |
| **Policy Compiler** | Parse policy declarations, compile `PolicyIR`, rewrite `PrimitiveGraph` (inserting `ApprovalNode` / `PolicyGuardNode`) | Schedule graph execution, perform LLM calls, bypass security rules |
| **Optimization Passes** | Apply graph transformations (dead node elimination, fan-out consolidation), annotate `NodeMetadata` | Alter execution semantics, introduce unvetted user intent |
| **Scheduler** | Lower `PrimitiveGraph` to `ExecutionGraph`, resolve readiness dependencies, dispatch work items | Rewrite graph topology, mutate capability contracts |
| **Connector Resolver** | Perform late binding of abstract `CapabilityInstance` handles to concrete `Connector` implementations | Modify graph node ordering, alter user security policies |
| **Plugin Executor** | Execute physical node logic (Rust native / WASM / dynamic libraries), emit telemetry | Mutate workflow graph structure, bypass node metadata bounds |

---

## Architecture Change Policy

Every proposed change to the codebase must clear the following 3-question governance check before implementation:

1. **Does it introduce a new abstraction?**
   * *Rule*: Requires an approved ADR.
2. **Does it modify a stable ABI (`CapabilityContract`, Plugin API, Scheduler API)?**
   * *Rule*: Requires an approved ADR + migration strategy.
3. **Does it violate ADR-027 phase invariants?**
   * *Rule*: Redesign required before implementation.
