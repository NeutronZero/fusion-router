# ADR-038: BudgetOptimisationPass Port Design

- **Status:** Accepted
- **Date:** 2026-08-09
- **Applies to:** `fusion-kernel` (trait), `fusion-compiler` (pass), monolith (`src/compiler`, `src/resource`)
- **Depends on:** ADR-034 (Single Compiler Pipeline), ADR-020 (Compiler Optimization Framework)

## Context

Wave 2 of the monolith-to-crate porting effort targets `BudgetOptimisationPass`. Unlike prior ports (ConstraintValidationPass, ControlFlowValidationPass, ModelResolutionPass, CapabilityResolver), this pass holds live shared state — an `Arc<dyn ResourceManager>` with atomic counters that persist *across* compiler invocations. The pass itself is thin (builds a throwaway `ExecutionGraph`, calls `can_afford()`), but the backing `DefaultResourceManager` has real lifecycle complexity.

The crate's `CompilerEngine` is explicitly a simulation (doc comment: "SIMULATION — Studio-sandbox compiler"). The production server is the monolith in `src/main.rs`. This asymmetry shapes the design.

## Design Questions and Decisions

### Q1: Where does the trait live, and who owns the running instance?

**Decision:** Trait definition in `fusion-kernel`. Running instance stays in the monolith.

- `ResourceManager` trait + `Quota` struct → `crates/fusion-kernel/src/resource/mod.rs` (parallel to `CapabilityRegistry` in `crates/fusion-kernel/src/capability/`)
- `DefaultResourceManager` → stays in `src/resource/mod.rs` (monolith owns the live instance)
- `CompilerEngine::new()` in `crates/fusion-compiler/` stays zero-arg — it's simulation-only, doesn't need real budget state
- `build_compiler()` in `src/compiler/mod.rs` already takes `Arc<dyn ResourceManager>` — no signature change needed
- **Compatibility ripple: zero.** No existing callers change.

### Q2: Does the crate need its own live budget state?

**Decision:** No. Stub/test-double for the crate; real state stays in monolith.

- The crate gets the trait definition only — no live instance, no injection
- `fusion-compiler` tests use a `StubResourceManager` (always returns `can_afford() = true`, zero counters)
- `DefaultResourceManager` with atomic counters remains monolith-internal
- **Scope note:** This makes the port legitimately smaller than it looks. The complexity was in `DefaultResourceManager`'s atomic counters, but that stays in the monolith for now. The crate gets: trait + stub + thin pass.

**Deferred to production cutover:** Wiring a live `Arc<dyn ResourceManager>` into the crate's `CompilerEngine`. That decision belongs to whoever starts serving real traffic from `apps/fusion-server`, not to this pass.

### Q3: What does equivalence mean for a stateful pass?

**Decision:** Construct matching state, then compare. New test pattern, but deterministic.

```
1. Create matching DefaultResourceManager instances (same Quota)
2. Simulate identical prior spend on both (same record_usage calls)
3. Build identical WorkflowIR input
4. Run the pass on both
5. Assert: both produce the same ExecutionGraph (or same error)
```

Test cases:
- `budget_pass_allows_under_quota` — IR fits within quota, both pass
- `budget_pass_rejects_over_quota` — IR exceeds quota, both reject
- `budget_pass_accumulates_spend` — two sequential passes, second sees accumulated state
- `budget_pass_shared_state` — two passes sharing one instance, second sees first's spend

## Consequences

- The trait becomes reusable by future crate-side code without coupling to the monolith's lifecycle
- The thin-pass-plus-stub pattern is the right default for simulation-only crates
- Equivalence tests for stateful passes are a new pattern — straightforward but distinct from the pure-function pattern used by prior passes
- The production cutover decision (wiring live state into the crate) is explicitly deferred, not accidentally defaulted into

## Scope

**In scope for this ADR:**
- `ResourceManager` trait + `Quota` → `fusion-kernel`
- `BudgetOptimisationPass` → `fusion-compiler`
- `StubResourceManager` for crate tests
- Equivalence tests with matching state setup

**Out of scope:**
- `DefaultResourceManager` stays in monolith — no change
- `CompilerEngine::new()` stays zero-arg — no change
- Wiring live state into the crate — deferred to production cutover

---

> **Governance note:** `docs/adr/` (uppercase) contains duplicate ADR numbers (two ADR-001s, duplicates at 002–008). This is pre-existing governance debt, not introduced by this ADR. `docs/adrs/` (lowercase) is the canonical directory for ADRs ≥ 017. Future ADRs should use `docs/adrs/` and sequential numbering from 039 onward.
