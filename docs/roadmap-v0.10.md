# FusionRouter v0.10 Roadmap — Capability Platform

> **Theme:** Make FusionRouter extensible. Everything outside the compiler becomes a capability.
> **Status:** Architectural Specification & Engineering Reference
> **Predecessor:** v0.9.0 (Compiler Pipeline & Deterministic Lowering)

---

## 1. Executive Vision & Structural Architecture

In **v0.8.0**, FusionRouter established the *Intent-Oriented Execution Model*.
In **v0.9.0**, FusionRouter solidified the compiler pipeline (`PrimitiveGraph` → `ExecutionGraph`, deterministic lowering, optimization passes, provenance, governance).

**v0.10.0** transforms FusionRouter from an orchestration engine into an open, extensible **Capability Platform** governed by compiler principles.

```text
                                STARTUP PHASE
                                ─────────────
                               Plugin Manager
                                      │
                                      ▼
                        Capability Registry (Immutable)

─────────────────────────────────────────────────────────────────────────────

                                RUNTIME PHASE
                                ─────────────
                             Request / Trigger
                                      │
                                      ▼
                             Requirements Extractor
                                      │
                                      ▼
                             Capability Resolver
                         (Symbol Resolution & CapabilityGraph)
                                      │
                                      ▼
                             CapabilityInstance
                         (Bound Runtime Execution Handle)
                                      │
                                      ▼
                             Planner (Intent & Workflow IR)
                                      │
                                      ▼
                             PrimitiveGraph (IR)
                                      │
                                      ▼
                             Policy Compiler Pass
                         (Auto-inserts Approval / Guard Nodes)
                                      │
                                      ▼
                             Optimization Passes
                         (Dead Node Elimination, Consolidation)
                                      │
                                      ▼
                             ExecutionGraph (Lowered IR with Node Metadata)
                                      │
                                      ▼
                             Scheduler / Runtime
                                      │
                                      ▼
                             Connector Resolver
                         (Late Binding of Capability to Connector)
                                      │
                                      ▼
                             CapabilityExecutor (Plugin Execution)
```

---

## 2. Core Architectural Refinements

1. **Immutable Runtime Registry**: `PluginManager` discovers and validates plugins during startup, builds the `CapabilityRegistry`, and **freezes** it (`Arc<CapabilityRegistry>`). The planner never mutates the registry at runtime.
2. **CapabilityContract as Semantic ABI**: `CapabilityContract` is the explicit semantic ABI between the Planner and Scheduler.
3. **CapabilityInstance Abstraction**: `CapabilityInstance` represents the bound runtime realization of a `CapabilityContract` (analogous to a compiled function pointer / bound execution handle).
4. **Capability Resolver Subsystem**: Capability resolution is separated into its own module (`src/planner/resolver/capability/`), treating resolution as symbol resolution.
5. **CapabilityGraph (Dependency DAG)**: Tracks capability dependencies, conflicts, and version constraints (e.g. Browser → Filesystem → Shell).
6. **Plugin API & Compiler Version Negotiation**: Plugins declare `PluginMetadata` (`api_version`, `min_compiler_version`) validated by a `CompatibilityChecker` at load time.
7. **Separation of Metadata vs. Execution**: `CapabilityContract` (metadata for Planner) is strictly decoupled from `CapabilityExecutor` (runtime for Scheduler).
8. **Planner Agnosticism & Late Binding Connectors**: Planner operates on abstract capabilities (`send_email`). The `Connector Resolver` performs late binding to concrete implementations (`gmail`, `outlook`) at execution time in the Scheduler.
9. **Declarative Policy Compilation**: Declarative policies are compiled into `PolicyIR` and processed by a compiler pass (`PolicyCompilerPass`) that automatically inserts `ApprovalNode` and `PolicyGuardNode` elements into `PrimitiveGraph`.
10. **Runtime Policies as Node Metadata Annotations**: Retry, timeout, approval, budget, and concurrency rules are attached as `NodeMetadata` annotations on graph nodes.
11. **Storage Engine Decoupling**: `ExecutionSession` runtime state management is decoupled from `SessionStore` (SQLite, Postgres, Memory, Redis).
12. **Capability Planner Cache**: LRU caching of `RequirementSet` → `ResolvedCapabilitySet` mappings accelerates planning throughput.
13. **Scoped Isolation Roadmap**:
    - **v0.10**: Rust C-ABI trait plugins (`libloading`), WASM plugins (`wasmtime`), static plugins.
    - **v0.11**: Out-of-process gRPC / IPC / Remote plugins.
    - **v1.0**: Distributed capability marketplace & production hardening.

---

## 3. Epics & Technical Specifications

### Epic A — Capability Registry & Immutable Resolver
- `CapabilityId`: Strongly-typed unique identifier (e.g., `github.issue.create`, `slack.send`, `browser.navigate`).
- `CapabilityContract`: Standard semantic ABI contract detailing input/output JSON schemas, required permissions, side effects, estimated cost, latency, reliability, determinism, and streaming support.
- `CapabilityRegistry`: Frozen, read-only thread-safe registry.
- `CapabilityResolver`: Dedicated symbol resolver module in `src/planner/resolver/capability/`.

---

### Epic B — Plugin SDK & Version Negotiation
- `fusion-plugin-api`: Public SDK crate.
- `PluginMetadata`: API versioning, compiler versioning, capability declarations.
- Lifecycle: `Discover` → `Validate` → `Load` → `Register` → `Activate`.
- Plugin types: `ProviderPlugin`, `CapabilityPlugin`, `StrategyPlugin`, `PolicyPlugin`, `OptimizerPlugin`.

---

### Epic C — Connector Framework & Late Binding Resolver
- `Connector`: Core trait for system integrations.
- `ConnectorResolver`: Binds abstract capability invocations to concrete connectors at execution time in the Scheduler.
- Reference connectors: GitHub, Slack, Discord, Jira, Notion, Filesystem, Browser, Shell, Email, Calendar.

---

### Epic D — Declarative Policy Compiler Engine
- `PolicyCompiler`: Compiles YAML/JSON policy rules into `PolicyIR`.
- `PolicyCompilerPass`: Compiler pass that auto-inserts `ApprovalNode` and `PolicyGuardNode` elements into `PrimitiveGraph`.

---

### Epic E — Trigger Framework
- `Trigger`: Emits standard `ExecutionRequest` structs into the Planner.
- Supported triggers: `ManualTrigger`, `CronTrigger`, `WebhookTrigger`, `EventTrigger`, `MessageTrigger`.

---

### Epic F — Session Runtime & Storage Decoupling
- `ExecutionSession`: Tracks session state, checkpoints, pause/resume, and cancellation.
- `SessionStore`: Decoupled trait with `MemorySessionStore`, `SqliteSessionStore`, `PostgresSessionStore`, and `RedisSessionStore` backends.

---

### Epic G — Capability Graph, Instance & Planner Cache
- `CapabilityGraph`: Dependency DAG for capabilities.
- `CapabilityInstance`: Bound runtime execution handle pairing `CapabilityContract` with runtime execution contexts.
- `CapabilityPlannerCache`: LRU cache mapping requirement hashes to resolved capability sets.

---

### Epic H — Node Metadata Annotations
- `NodeMetadata`: Graph node annotations for `retry`, `timeout`, `approval`, `budget`, `concurrency`, and `security_guards`.

---

### Epic I — Public SDK Crates Workspace Architecture
- Public workspace crates: `fusion-plugin-api`, `fusion-capability`, `fusion-connector`, `fusion-policy`.

---

### Epic J — Reference Plugins Ecosystem
- Reference implementations: `fusion-plugin-github`, `fusion-plugin-filesystem`, `fusion-plugin-shell`, `fusion-plugin-browser`, `fusion-plugin-http`, `fusion-plugin-mcp`, `fusion-plugin-slack`.

---

### Epic K — Capability Discovery CLI
- CLI commands (`cargo fusion plugins`): `list`, `inspect`, `verify`, `install`.

---

### Epic L — Capability Marketplace & Manifest Specification
- `PluginManifest`: Formal specification including publisher identity, cryptographic signature, license, and capability dependencies.

---

## 4. Architectural Decision Records (ADR) Matrix

| ADR | Title | Key Architectural Decision |
|---|---|---|
| [ADR-021](file:///c:/Projects/fusion-router/docs/adr/ADR-021-capability-platform.md) | Capability Platform Architecture | Immutable `CapabilityRegistry` at runtime, startup vs runtime phase separation |
| [ADR-022](file:///c:/Projects/fusion-router/docs/adr/ADR-022-plugin-abi.md) | Plugin SDK & Version Negotiation | `PluginMetadata` version checks, strict separation of metadata (`CapabilityContract`) vs execution (`CapabilityExecutor`) |
| [ADR-023](file:///c:/Projects/fusion-router/docs/adr/ADR-023-capability-resolution.md) | Capability Resolution & CapabilityInstance | `planner::resolver::capability` subsystem, `CapabilityGraph` dependency DAG, `CapabilityInstance` bound handle |
| [ADR-024](file:///c:/Projects/fusion-router/docs/adr/ADR-024-policy-compilation.md) | Policy Compilation | Declarative policies compiled into `PolicyIR`, `PolicyCompilerPass` auto-inserting approval nodes, `NodeMetadata` annotations |
| [ADR-025](file:///c:/Projects/fusion-router/docs/adr/ADR-025-connector-abstraction.md) | Connector Abstraction | Planner plans abstract capabilities; `ConnectorResolver` performs late binding at execution time |
| [ADR-026](file:///c:/Projects/fusion-router/docs/adr/ADR-026-execution-session.md) | Execution Session Runtime | `ExecutionSession` decoupled from `SessionStore` backends (SQLite, Postgres, Memory, Redis) |

---

## 5. Vision for v1.0.0 (Production Platform Hardening)

With v0.10.0 establishing the extensible Capability Platform architecture, **v1.0.0** will focus exclusively on production readiness and ecosystem guarantees rather than introducing structural redesigns:
- **SDK Stability Guarantees**: Freeze `fusion-plugin-api` v1.0.0.
- **Conformance & Certification Test Suites**: Conformance test suites for plugins, policies, and connectors.
- **Execution Replay & Provenance Tooling**: Offline replay of execution sessions using compiler provenance hashes.
- **Security Hardening**: Cryptographic plugin signing, WASM sandboxing audit, and permission isolation models.
