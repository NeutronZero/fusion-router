# FusionRouter v0.10 Roadmap — Capability Platform

> **Theme:** Make FusionRouter extensible. Everything outside the compiler becomes a capability.
> **Status:** Architecture Frozen & Implementation Phase
> **Predecessor:** v0.9.0 (Compiler Pipeline & Deterministic Lowering)

---

## 1. Executive Vision & The Three Interfaces of FusionRouter

In **v0.8.0**, FusionRouter established the *Intent-Oriented Execution Model*.
In **v0.9.0**, FusionRouter solidified the compiler pipeline (`PrimitiveGraph` → `ExecutionGraph`, deterministic lowering, optimization passes, provenance, governance).

**v0.10.0** transforms FusionRouter into a compiler-oriented **Capability Platform**, cleanly separating three primary interfaces:

1. **Compiler API (Internal)**: `Intent` → `Planner` → `PrimitiveGraph` → `ExecutionGraph`. Engine implementation detail.
2. **Capability ABI (`fusion-plugin-api v0.1.0`)**: `CapabilityContract` → `CapabilityInstance` → `CapabilityExecutor`. Independent, lightweight SDK crate.
3. **Runtime API (Application Surface)**: OpenAI-compatible `/v1/chat/completions`, REST endpoints, and streaming channels.

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

## 2. Minimal `fusion-plugin-api v0.1.0` SDK Surface

To prevent SDK bloat, `fusion-plugin-api` is an independent workspace crate (`fusion-plugin-api v0.1.0`) exposing strictly minimal surface area:

```rust
pub struct CapabilityId(pub String);

pub struct PluginMetadata {
    pub name: String,
    pub version: semver::Version,
    pub api_version: semver::Version,
    pub min_compiler_version: semver::Version,
    pub capabilities: Vec<CapabilityId>,
}

pub struct CapabilityContract {
    pub id: CapabilityId,
    pub version: semver::Version,
    pub inputs_schema: serde_json::Value,
    pub outputs_schema: serde_json::Value,
    pub permissions: Vec<String>,
    pub estimated_cost_usd: f64,
    pub estimated_latency_ms: u64,
    pub reliability_score: f32,
    pub supports_streaming: bool,
}

pub struct CapabilityInstance {
    pub contract: CapabilityContract,
    pub runtime_params: serde_json::Value,
}

pub trait Plugin: Send + Sync {
    fn metadata(&self) -> PluginMetadata;
}

pub trait CapabilityPlugin: Plugin {
    fn capabilities(&self) -> Vec<CapabilityContract>;
}

pub trait CapabilityExecutor: Send + Sync {
    async fn execute(&self, instance: &CapabilityInstance, input: serde_json::Value) -> Result<serde_json::Value, String>;
}
```

---

## 3. Phase 1 Acceptance Criteria & Minimal Reference Plugin

### Acceptance Criteria:
- `fusion-plugin-api` builds as an isolated workspace crate.
- Minimal reference plugin `plugins/fusion-plugin-echo` compiles relying **only** on `fusion-plugin-api`.
- `PluginManager` discovers `fusion-plugin-echo`.
- `CompatibilityChecker` validates API version (`v0.1.0`) and compiler version compatibility.
- `CapabilityRegistry` registers contracts (`echo.text`, `echo.uppercase`) and freezes successfully (`Arc<CapabilityRegistry>`).
- Planner can query frozen capabilities from the registry.

---

## 4. Compiler Phase Invariants (ADR-027)

As specified in [ADR-027](file:///c:/Projects/fusion-router/docs/adr/ADR-027-compiler-phase-invariants.md) and [docs/architecture/invariants.md](file:///c:/Projects/fusion-router/docs/architecture/invariants.md):

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

## 5. Bottom-Up Implementation Roadmap

1. **Phase 1 — Foundations**: `fusion-plugin-api v0.1.0`, `CapabilityContract`, `CapabilityRegistry`, `PluginManager`, Registry freeze, Compatibility checker, `fusion-plugin-echo`.
2. **Phase 2 — Resolution**: `CapabilityGraph`, `CapabilityResolver`, `CapabilityInstance`, Planner integration.
3. **Phase 3 — Runtime**: `ConnectorResolver`, `CapabilityExecutor`, Reference plugins (GitHub, MCP, Shell, Browser, Slack), Scheduler integration.
4. **Phase 4 — Compiler**: `PolicyIR`, `PolicyCompilerPass`, Node metadata annotations.
5. **Phase 5 — Sessions**: `ExecutionSession`, `SessionStore`, Checkpointing, Resume, Cancellation.
6. **Phase 6 — Triggers**: Manual, Cron, Webhook, Event, Message triggers.
7. **Phase 7 — Tooling & Verification**: Plugin CLI (`cargo fusion plugins`), Verification tools, Marketplace manifests, Conformance tests.

---

## 6. Architectural Decision Records (ADR) Matrix

| ADR | Title | Key Architectural Decision |
|---|---|---|
| [ADR-021](file:///c:/Projects/fusion-router/docs/adr/ADR-021-capability-platform.md) | Capability Platform Architecture | Immutable `CapabilityRegistry` at runtime, startup vs runtime phase separation |
| [ADR-022](file:///c:/Projects/fusion-router/docs/adr/ADR-022-plugin-abi.md) | Plugin SDK & Version Negotiation | `PluginMetadata` version checks, strict separation of metadata (`CapabilityContract`) vs execution (`CapabilityExecutor`) |
| [ADR-023](file:///c:/Projects/fusion-router/docs/adr/ADR-023-capability-resolution.md) | Capability Resolution & CapabilityInstance | `planner::resolver::capability` subsystem, `CapabilityGraph` dependency DAG, `CapabilityInstance` bound handle |
| [ADR-024](file:///c:/Projects/fusion-router/docs/adr/ADR-024-policy-compilation.md) | Policy Compilation | Declarative policies compiled into `PolicyIR`, `PolicyCompilerPass` auto-inserting approval nodes, `NodeMetadata` annotations |
| [ADR-025](file:///c:/Projects/fusion-router/docs/adr/ADR-025-connector-abstraction.md) | Connector Abstraction | Planner plans abstract capabilities; `ConnectorResolver` performs late binding at execution time |
| [ADR-026](file:///c:/Projects/fusion-router/docs/adr/ADR-026-execution-session.md) | Execution Session Runtime | `ExecutionSession` decoupled from `SessionStore` backends (SQLite, Postgres, Memory, Redis) |
| [ADR-027](file:///c:/Projects/fusion-router/docs/adr/ADR-027-compiler-phase-invariants.md) | Compiler Phase Invariants | Constitution defining May Do vs Must Not Do per phase |
| [ADR-028](file:///c:/Projects/fusion-router/docs/adr/ADR-028-capability-contract-evolution.md) | Capability Contract Evolution | Semver rules, aliasing, deprecation grace periods, and feature fallbacks |
