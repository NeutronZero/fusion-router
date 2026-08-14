# Architectural Constitution & Convergence Plan (Frozen Canonical Edition)

This document is the **frozen architectural constitution** for FusionRouter. It serves as the engineering authority for the convergence phase, governing authority, ownership, invariants, forbidden fallbacks, deterministic compilation, and enforceable CI gates.

---

## Core Architectural Invariants & Laws

### 1. The Host Boundary Law (`src/` as an Anti-Authority Zone)
> **`src/` may translate, orchestrate, persist, expose, or adapt; it may NOT make an independent execution-planning, model selection, strategy lowering, provider selection, or runtime execution decision.**

All execution-plane decisions belong strictly to the dedicated crates (`fusion-planner`, `fusion-ir`, `fusion-compiler`, `fusion-scheduler`, `fusion-runtime`).

### 2. Dependency-Safe Canonical Monetary Type (`NanoUSD`)
- **Location**: `fusion_core::monetary::NanoUSD(pub u64)` where $1 \text{ NanoUSD} = 10^{-9} \text{ USD}$.
- **Dependency Invariant**: `fusion-core::monetary` must have **zero dependencies** on any crate that consumes `NanoUSD`.
  ```text
  fusion-core
      ↓
  fusion-ir
      ↓
  fusion-planner, fusion-types, fusion-compiler, fusion-scheduler,
  fusion-runtime, fusion-plugin-api, fusion-capability-sdk
  ```
- **Arithmetic Surface & Conversion Boundary**:
  - Implements `Add`, `Sub`, `checked_add`, `checked_sub`, `saturating_add`, `saturating_sub`, `checked_from_decimal_usd()`, `to_decimal_usd()`.
  - Conversion from decimal USD (`$0.1234567894`) requires exact parsing or explicit error rejection (`checked_from_decimal_usd()`). Silent downward rounding or overflow is prohibited.
  - Prohibits internal floating-point monetary fields (`f64`).

### 3. Single Authoritative `ExecutionIntent` & Snapshot-Only `PlanningRequest`
- Single Intent Definition (`fusion_planner::ExecutionIntent`):
  ```rust
  pub enum ExecutionIntent {
      Quality,
      Speed,
      Balanced,
      Exhaustive,
      Constrained { max_cost: Option<NanoUSD> },
  }
  ```
- Snapshot-Only Planning Boundary: `fusion-planner` consumes immutable snapshot schemas (`PolicySnapshot`, `ModelCatalogSnapshot`, `CapabilityCatalogSnapshot`, `RoutingTelemetrySnapshot`) and never receives live service handles (`Arc<Mutex<...>>` or DB handles).
  ```rust
  pub struct PlanningRequest {
      pub intent: ExecutionIntent,
      pub user_prompt: String,
      pub requested_model: Option<String>,
      pub requirements: Requirements,
      pub policies: PolicySnapshot,
      pub capability_catalog: CapabilityCatalogSnapshot,
      pub model_catalog: ModelCatalogSnapshot,
      pub telemetry: RoutingTelemetrySnapshot,
  }
  ```

### 4. Authoritative ModelCatalog & PolicyRegistry Control Plane
- Model Authority: `ConfigManager` $\rightarrow$ `ModelCatalogSnapshot` $\rightarrow$ `PlanningRequest` $\rightarrow$ `fusion-planner`. No host code mutates model assignments post-planning.
- Policy Authority: `src/policy/policy_registry.rs` owns mutable `PolicyRegistry`; `PolicyAdmin` acts as an administrative facade calling `PolicyRegistry`.
- `PolicyRegistry` emits immutable `PolicySnapshot { version, policies, created_at }`. `policy_snapshot_version: u64` is recorded in `WorkflowIR`, `ExecutionGraph`, and telemetry/evidence for reproducible replay.

### 5. Total Strategy Compilation & Zero Fallback Rule
- Every `StrategyKind` (`Single`, `Consensus`, `Reflection`, `Chain`, `Debate`, `ReAct`, `Fusion`, `Custom`) lowers to `Lowered(ExecutionGraph)` or returns explicit `CompileError`. `passthrough`, `unimplemented`, or default strategy fallbacks (`_ => passthrough(...)`) are strictly forbidden architectural violations.
- `Custom` strategies compile via explicit plugin delegate trait:
  ```rust
  pub trait StrategyCompiler {
      fn compile(&self, strategy: &CustomStrategy, context: &CompilationContext) -> Result<ExecutionGraph, CompileError>;
  }
  ```

### 6. Authoritative Streaming & SSE Transport Adapter
- `/v1/chat/completions?stream=true` executes through `PlanningRequest` $\rightarrow$ `fusion-ir` $\rightarrow$ `fusion-compiler` $\rightarrow$ `fusion-scheduler` $\rightarrow$ `fusion-runtime`.
- SSE streaming is strictly an **output transport adapter**.
- Direct provider streaming is gated behind `FUSION_EXPERIMENTAL_DIRECT_STREAM=true` at process startup; cannot be enabled via reload, request parameters, or headers.

---

## Phased Execution Sequence

```text
Phase A: NanoUSD Core Definition + Single ExecutionIntent + PlanningRequest
        │
Phase B: PlanningExecutionPreservation Conformance Suite
        │
Phase C: Total Strategy Expansion & StrategyCompiler Contract
        │
Phase D: Runtime Execution Convergence
        │
Phase E: Control-Plane Wiring (Authoritative PolicyRegistry & Plugin Lifecycle)
        │
Phase F: Streaming Convergence & SSE Transport Adapter
        │
Phase G: Repository-Wide NanoUSD Migration (including SDK & Plugin API)
        │
Phase H: Legacy Deletion & 11-Gate Repository-State Firewall
```

---

## File Modifications

### Phase A — `NanoUSD` & `PlanningRequest`

#### [NEW] [monetary.rs](file:///c:/Projects/fusion-router/crates/fusion-core/src/monetary.rs)
- Implement `NanoUSD` with complete overflow-safe arithmetic and exact decimal conversion surface.

#### [NEW] [planning_request.rs](file:///c:/Projects/fusion-router/crates/fusion-planner/src/planning_request.rs)
- Define standalone `ExecutionIntent` and `PlanningRequest` with snapshot schemas (`PolicySnapshot`, `ModelCatalogSnapshot`, `CapabilityCatalogSnapshot`, `RoutingTelemetrySnapshot`).

#### [DELETE] [dynamic_planner.rs](file:///c:/Projects/fusion-router/src/planner/dynamic_planner.rs)
#### [DELETE] [simple.rs](file:///c:/Projects/fusion-router/src/planner/simple.rs)
#### [MODIFY] [intent_planner.rs](file:///c:/Projects/fusion-router/src/planner/intent_planner.rs)
- Remove all host-side fallback planning logic (`build_quality`, `build_speed`, `build_balanced`, `build_exhaustive`). Delegate strictly to `fusion_planner`.

#### [MODIFY] [chat.rs](file:///c:/Projects/fusion-router/src/server/handlers/chat.rs)
- Construct `PlanningRequest` with requested model as input constraint. Eliminate post-planning model mutation.

---

### Phase B — `PlanningExecutionPreservation` Conformance

#### [MODIFY] [adapter.rs](file:///c:/Projects/fusion-router/src/ir/adapter.rs)
- Preserve semantic meanings across compilation boundary for all 9 `fusion-ir` kinds: `Task`, `Tool`, `Retrieval`, `Memory`, `Review`, `Judge`, `Security`, `Aggregation`, `Output`.

#### [NEW] [planning_execution_preservation.rs](file:///c:/Projects/fusion-router/tests/planning_execution_preservation.rs)
- Implement preservation conformance test validating semantic invariants (stable identity, semantic capabilities, model constraints, policy versions, strategies).

---

### Phase C — Total Strategy Compiler Lowering

#### [NEW] [strategy_compiler.rs](file:///c:/Projects/fusion-router/crates/fusion-compiler/src/strategy_compiler.rs)
- Implement total strategy lowering across all 8 variants with zero passthrough fallbacks.

#### [DELETE] [legacy_passes.rs](file:///c:/Projects/fusion-router/src/compiler/passes/legacy_passes.rs)
#### [DELETE] [strategy_expansion.rs](file:///c:/Projects/fusion-router/src/compiler/strategy_expansion.rs)

---

### Phase D — Runtime Convergence

#### [MODIFY] [node_exec.rs](file:///c:/Projects/fusion-router/src/executor/node_exec.rs)
#### [DELETE] [strategy_resolver.rs](file:///c:/Projects/fusion-router/src/executor/strategy_resolver.rs)
- Remove `src/executor` fallback strategies. Consolidate execution inside `fusion_runtime::ProviderExecutor` and `fusion_runtime::ToolRegistry`.

---

### Phase E — Authoritative PolicyRegistry & Production Wiring

#### [NEW] [policy_registry.rs](file:///c:/Projects/fusion-router/src/policy/policy_registry.rs)
- Implement `PolicyRegistry` emitting versioned immutable `PolicySnapshot`.

#### [MODIFY] [policy_admin.rs](file:///c:/Projects/fusion-router/src/operations/policy_admin.rs)
- Refactor `PolicyAdmin` as a thin administrative facade over `PolicyRegistry`.

#### [MODIFY] [main.rs](file:///c:/Projects/fusion-router/src/main.rs)
- Wire `ArchivePackageVerifier` with `FilesystemArchiveBackend` and `Signer`. Add production assertion forbidding `MockPackageVerifier`.
- Wire `PluginManager` startup lifecycle (`Discover` $\rightarrow$ `Load` $\rightarrow$ `Validate` $\rightarrow$ `Initialize` $\rightarrow$ `Register` $\rightarrow$ `Activate`).

---

### Phase F — SSE Streaming Pipeline

#### [MODIFY] [chat.rs](file:///c:/Projects/fusion-router/src/server/handlers/chat.rs)
- Route `stream=true` through `PlanningRequest` $\rightarrow$ `fusion-ir` $\rightarrow$ `fusion-compiler` $\rightarrow$ `fusion-scheduler` $\rightarrow$ `fusion-runtime` with SSE transport chunking.

---

### Phase G — Full `NanoUSD` Migration

#### [MODIFY] [fusion-plugin-api / fusion-capability-sdk / fusion-ir / fusion-types / fusion-compiler / fusion-scheduler / fusion-runtime / resource / telemetry]
- Replace all internal monetary `f64` and `u64` millicost fields with `NanoUSD`.

---

### Phase H — 11-Gate Repository Firewall

#### [NEW] [production_wiring.rs](file:///c:/Projects/fusion-router/tests/production_wiring.rs)
#### [NEW] [verifier.rs / policy.rs / plugins.rs / streaming.rs](file:///c:/Projects/fusion-router/tests/production_wiring/)
- Integration tests verifying production binary component wiring.

#### [MODIFY] [check_monolith_freeze.py](file:///c:/Projects/fusion-router/scripts/check_monolith_freeze.py)
- Implement repository-state AST/text scanner enforcing all 11 Convergence Gates:
  1. **Gate 01 Planner Authority**: Zero host planner implementations (`build_quality`, `dynamic_planner`, `simple.rs` absent).
  2. **Gate 02 Compiler Authority**: Zero host compiler passes remaining in `src/compiler`.
  3. **Gate 03 Strategy Authority**: Zero host strategy execution in `src/executor`.
  4. **Gate 04 Runtime Authority**: Zero legacy provider execution paths.
  5. **Gate 05 Attestation Authority**: Zero production `MockPackageVerifier` usages (`#[cfg(test)]` only).
  6. **Gate 06 Policy Authority**: `PolicyRegistry` is the single authoritative policy source.
  7. **Gate 07 Capability Authority**: Single authoritative `CapabilityRegistry` / `PluginManager` source.
  8. **Gate 08 Streaming Authority**: Streaming and non-streaming share standard `ExecutionGraph`.
  9. **Gate 09 Monetary Authority**: Zero internal `f64` / millicost monetary fields in crates or SDKs.
  10. **Gate 10 Fallback Elimination**: Zero passthrough or strategy fallbacks in compiler/runtime.
  11. **Gate 11 Total Determinism**: Zero entropy sources in planning/compilation (no random UUID v4, no unordered HashMap iteration, no time/address derived identities).

---

## Verification Plan

### Automated Tests
- `cargo test --workspace`
- `cargo test --test planning_execution_preservation`
- `cargo test --test production_wiring`
- `python scripts/check_monolith_freeze.py` (Asserts `ARCHITECTURE STATUS: CONVERGED`)

### Manual Verification
- Query `GET /v1/operations/attestations` for real verifier state.
- Issue `stream=true` request and verify evidence logging in `fusion_telemetry.db`.
