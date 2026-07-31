# FusionRouter v0.13.0 — AI Execution Compiler — Architecture Specification (Frozen)

- **Status:** Frozen
- **Version:** 0.13.0
- **Date:** 2026-07-31
- **Classification:** Public contract. Implementation follows this document; this document does not follow implementation.

## Vision

FusionRouter is an **AI execution compiler** — it compiles high-level user intent into executable, verifiable, replayable workflow artifacts. Execution is not a prediction; it is a compilation product. The system turns natural language into executable work through explicit compiler stages, each producing a verifiable artifact, exactly as a traditional compiler turns source code into machine code.

## Architectural Philosophy

- **Execute, don't predict.** No auto-execution, no silent decisions, no invisible state. Every execution decision is compiled, explicit, and recorded.
- **Compile, don't instruct.** The planner and compiler produce artifacts — never loose instructions for a runtime to interpret freely.
- **Explicitness over cleverness.** Hidden inference is an architectural defect. If a behavior is not expressed in an artifact, it does not exist.
- **Resilience to provider churn.** The architecture must survive the death of any individual capability, provider, or model vendor. No contract above the Runtime layer may name one.

## Core Principles

1. **User intent is source code.** Intent is the input program; everything downstream is compilation of that program.
2. **Plans are code.** `WorkflowIR` is a first-class artifact: immutable, versioned, serializable, auditable.
3. **Only the compiler generates an ABI.** A runtime never invents its own executable interpretation of a plan.
4. **Every ABI targets an explicit execution target.** Compilation is placement-aware; there is no unplaced executable work.
5. **Provider independence above the runtime.** Nothing above the Runtime layer may reference a concrete model, provider, endpoint, or transport.
6. **Determinism and verifiability.** Same input yields the same executable artifact; every artifact is verifiable by construction.
7. **Semantic equivalence under optimization.** Optimizations may change cost, latency, or resource usage — never meaning.
8. **Capabilities are the atomic unit of execution.** Everything that runs is a capability invocation; the registry is the source of semantic truth.
9. **Every execution is auditable and replayable.** Evidence is a compile-time and runtime invariant, not an afterthought.
10. **Compilation is the only path to execution.** There is exactly one way for intent to become running work: the compiler pipeline.

## Six Core Abstractions

The project identity is defined by exactly six stable abstractions. All future development integrates through them; no new foundational concept may be introduced without an ADR (see Architectural Law 4 below and ADR-033).

1. **`NormalizedIntent`** — the canonical, provider-free representation of user goals and constraints. Immutable after normalization. Contract 1.
2. **`WorkflowIR`** — the provider-independent logical workflow. A versioned, immutable graph of semantic nodes and edges. Contract 2.
3. **`ExecutionAbi`** — the executable workflow contract between compiler and runtime. Only the compiler may generate it. Contract 3.
4. **`ExecutionTarget`** — provider-independent runtime placement and environment constraints. Contract 4.
5. **`ExecutionRuntimeInterface` (ERI)** — the stable runtime execution contract. The runtime executes ABIs; it never interprets user intent. Contract 5.
6. **`CapabilityRegistry` + `CapabilityTrait`** — the provider-independent semantic capability catalog, enriched with execution-relevant traits. Contract 6.

## Layers

### Layer 1: Intent & Semantics

Owns meaning: intent normalization, capability semantics, memory, retrieval, policy constraints, budgets. Produces `NormalizedIntent` and resolves semantic capabilities. No executable work exists in this layer, and no provider exists in this layer.

### Layer 2: Compilation

Owns transformation: planning (`NormalizedIntent` → `WorkflowIR`), the compiler pass pipeline, optimization, and ABI generation (`WorkflowIR` → `ExecutionAbi` for an `ExecutionTarget`). The compiler is deterministic, side-effect-free, and performs no LLM, filesystem, or network I/O. Optimization preserves semantic equivalence (Law 9).

### Layer 3: Runtime

Owns execution: scheduler, executor, provider resolution, sandboxing, telemetry, and evidence. The runtime consumes only `ExecutionAbi` plus `ExecutionTarget`; provider binding happens here and below, never above. The runtime never plans, never optimizes semantically, and never rewrites the ABI.

## Execution ABI

`ExecutionAbi` represents executable work rather than logical work. It is the stable, versioned contract between the compiler and the runtime:

- Only the compiler may generate it (Law 4).
- It is provider-free: nodes reference capabilities, never models (Law 7).
- It is versioned (`EXECUTION_ABI_VERSION`); version mismatches are explicit errors.
- It is immutable from the moment the compiler emits it until execution completes.
- It is generated for a specific `ExecutionTarget` (Law 5).

## Execution Target

`ExecutionTarget` describes where and how work executes, without naming any provider: execution environment (Local, Cloud, Kubernetes, GpuCluster, Edge, AirGapped, Browser, Hybrid), resource limits (memory, CPU, parallelism), network constraints (egress policy, allowed domains), security profile (sandbox requirement, attestation requirement), and preferred scheduler (DepthFirst, BreadthFirst, CriticalPath, LatencyOptimized, CostOptimized, Distributed).

## Execution Runtime Interface (ERI)

The ERI is the runtime-side contract. It exposes exactly: runtime identity, `execute(abi, target) → result`, `cancel(execution_id)`, and `state(execution_id)`. The ERI is object-safe, `Send + Sync`, and does not expose intent, planning, or provider surfaces. The runtime behind the ERI owns provider resolution, sandboxing, and evidence collection.

## Runtime

The runtime executes ABIs against targets. It is a faithful execution engine, not a decision-maker: it schedules the immutable graph, dispatches nodes as capability invocations, applies per-node policies (retry, cache, security, evaluation), records telemetry, and reports state transitions through the nine-state model. The runtime may resolve capabilities late — at dispatch time — because provider binding is runtime-owned.

## Capability Registry

The registry is the semantic catalog of everything that can execute: capabilities with IDs, versions, JSON schemas for inputs/outputs, permissions, dependencies, cost and latency estimates, reliability scores, streaming support, and semantic traits (`CapabilityTrait`: Streaming, LongContext, StructuredOutput, LowLatency, DeterministicOutput, ComputerUse). The registry is frozen after registration (mutable-then-frozen). Capabilities, not models, are what the compiler binds into ABIs.

## Execution Node

The execution node is the unit of ABI work. Its contract is fixed: node ID, role, capability reference, declared inputs and outputs, constraints (max latency, max cost, max tokens), reasoning budget (max tokens, max steps), retry policy (max retries, backoff), cache policy (TTL, key hint), security policy (sandbox and validation requirements), evaluation policy (faithfulness, relevance, tool correctness), and telemetry hooks. Nodes may also be non-capability control nodes (aggregation, output, gate) per graph structure.

## Runtime Infrastructure

- **Scheduler:** topology-driven execution of the immutable ABI graph; picks ready nodes by target-appropriate policy; owns no semantic decisions.
- **Sandboxing:** capability invocations execute under declared security policies (sandbox, validation).
- **Module cache:** compiled capability modules are cached; the registry is immutable after freeze.
- **Evidence & audit:** every state transition and result is recorded to the evidence repository; execution is replayable from recorded evidence.

## Scheduler

The scheduler reads immutable executable work and mutates only runtime state. It supports depth-first, breadth-first, critical-path, latency-optimized, cost-optimized, and distributed strategies, selected by the `ExecutionTarget` preference. It never re-plans and never re-orders for semantic reasons.

## Cost Model

Cost is first-class, declared at every level: `Budget` on `NormalizedIntent`, `AbiConstraints.max_cost_usd` and `WorkflowMetadata.estimated_cost` on IR, and per-capability cost estimates in the registry. The runtime enforces budgets; the compiler and planner reason over them.

## Compiler Context

The compiler receives an immutable context: the normalized intent, the workflow IR, the policy surface, the capability registry, and the target. Compilation is a pure function of this context. No context mutation is permitted during compilation.

## Optimization Levels

The compiler may apply optimizations (node fusion, budget optimization, retry/fallback insertion, scheduling hints) at declared levels. All optimizations preserve semantic equivalence (Law 9): they may change cost, latency, parallelism, or resource usage, but never the meaning of the program.

## Memory

Memory is semantic state attached to intents and sessions: session snapshots, checkpoints, and evidence. Memory is provider-free; retrieval is a capability.

## Retrieval

Retrieval is a first-class node kind in `WorkflowIR` and a capability at runtime. Intent and IR refer to retrieval semantically; the concrete retrieval provider is a runtime resolution.

## Security

Security is enforced by policy compilation, per-node security policies, sandbox requirements, validation requirements, and attestation at the target level. Security decisions are compiled into the ABI — never improvised at runtime.

## Evaluation

Evaluation is compiled, not ad hoc: faithfulness, relevance, and tool-correctness policies attach to ABI nodes. Evaluation outcomes are evidence, recorded like all other execution output.

## Telemetry

Telemetry never blocks execution. Events, metrics, and traces are first-class records; audit log and evidence repository are part of the execution contract, not observability add-ons.

## Adaptive Loop

Feedback calibrates the compiler inputs, not the runtime. Outcomes of execution become evidence that feeds intent normalization, planning, and cost estimation. The runtime never adapts silently.

## Execution State Model

Execution passes through exactly nine states:

`Planned → Compiled → Queued → Running → Waiting → Retrying → Succeeded / Failed / Cancelled`

Every transition is recorded; every state is queryable through the ERI; every terminal state carries evidence.

## Graph Templates

Reusable, versioned subgraph patterns (retrieval-augmented generation, reflection, review-judge loops, consensus, debate, chain, fusion) are expressible as `WorkflowIR` fragments with exactly one entry and one exit. Templates are compiled like any other program; they are not runtime magic.

## Frontend Strategy

The compiler core is exposed through thin surfaces: HTTP server, CLI, SDK, and event streams. Frontends translate user input to `NormalizedIntent`; they never construct executable work directly.

## Plugin Architecture

Capabilities are distributed as versioned packages (`.fusionpkg`) and loaded through the plugin API (`fusion-plugin-api`), macros, and SDK crates. Plugins declare capabilities, permissions, and traits; the compiler and runtime consume them through the registry only. The plugin ABI is a separate, versioned contract (`CAPABILITY_ABI_VERSION`).

## Repository Layout

```text
src/intent        NormalizedIntent (contract 1)
src/ir            WorkflowIR (contract 2)
src/compiler      Compiler pipeline, PrimitiveGraph (compiler-internal), ABI generator (v0.14)
src/abi           ExecutionAbi (contract 3)
src/target        ExecutionTarget (contract 4)
src/eri           ExecutionRuntimeInterface (contract 5)
src/capability    CapabilityRegistry (contract 6)
src/runtime       Runtime behind the ERI
src/package       .fusionpkg package platform
src/operations    Operations platform
crates/           Plugin API, capability macros, capability SDK
plugins/          Example and echo capability plugins
```

## Architectural Laws

1. **Compilation is the only path to execution.** No component may execute work that did not pass through the compiler.
2. **User intent is source code.** Intent is the input program; it is compiled, never improvised upon.
3. **Plans are code; execution is a compiler's job.** `WorkflowIR` is immutable, versioned, and serializable.
4. **Only the compiler generates an ABI.** ABI generation is a compiler-exclusive act; runtimes never synthesize executable work.
5. **Every ABI targets an explicit execution target.** Unplaced executable work is invalid.
6. **Every execution is auditable and replayable.** Evidence is a contract, not an add-on.
7. **Above the runtime, there is no provider.** No model, provider, endpoint, or transport above Layer 3.
8. **Provider code never touches the intent path.** Provider-specific logic operates at or below runtime dispatch.
9. **Optimizations preserve semantic equivalence.** Compiler transforms never change program meaning.
10. **Capabilities are the atomic unit of execution.** Everything that runs is a capability invocation resolved through the registry.

## Roadmap

- **v0.13.0 — Architecture Freeze.** Six core abstractions frozen as stable public contracts; specification published; ADRs recorded. (This milestone.)
- **v0.14.0 — Compiler Core.** `WorkflowIR` implementation, `ExecutionAbi` v1, compiler pass framework, capability registry, local runtime behind the ERI.
- **v0.15.0 — Planner.** Intent normalization and planning onto the frozen contracts.
- Thereafter: distributed runtime, advanced optimization, ecosystem SDK — all built on the frozen contracts.

## LLVM Analogy

| FusionRouter | LLVM |
|---|---|
| User intent | Source code |
| `NormalizedIntent` | Language front-end output |
| `WorkflowIR` | LLVM IR |
| Compiler passes | Optimization pipeline |
| `PrimitiveGraph` | Compiler-internal representation |
| ABI generator | Code generator |
| `ExecutionAbi` | Machine code |
| `ExecutionTarget` | Target triple |
| `ExecutionRuntimeInterface` | Instruction set architecture |
| `CapabilityRegistry` | System call interface |
| Runtime | Hardware / OS |

## One-Sentence Definition

FusionRouter is a compiler that compiles user intent into executable, verifiable, replayable workflow artifacts — and a runtime that executes them faithfully against explicit targets, through a provider-independent capability registry.

## Architecture Freeze Declaration

This architecture is declared frozen as of v0.13.0. The six core abstractions — `NormalizedIntent`, `WorkflowIR`, `ExecutionAbi`, `ExecutionTarget`, `ExecutionRuntimeInterface`, and `CapabilityRegistry` with `CapabilityTrait` — are stable public contracts. Changes to these contracts require a new ADR and a new architecture freeze. The v0.12 implementation remains functional behind compatibility boundaries; reconciling it to these contracts is v0.14 boundary work, and no future feature may introduce a new foundational concept outside the six abstractions.
