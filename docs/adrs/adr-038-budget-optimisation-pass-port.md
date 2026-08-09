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

**Decision:** No live instance, but a state-aware stub for testing.

- The crate gets the trait definition only — no live instance, no injection
- `fusion-compiler` tests use a `StubResourceManager` that **tracks state** via `AtomicU64` counters and checks against quota in `can_afford()` (not a dumb `true` return)
- `DefaultResourceManager` with atomic counters remains monolith-internal
- **Scope note:** This makes the port legitimately smaller than it looks. The complexity was in `DefaultResourceManager`'s reservation/release semantics, but that stays in the monolith for now. The crate gets: trait + state-aware stub + thin pass.

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

### Q4: How many methods should the `ResourceManager` trait have?

**Decision:** 7 methods — the full monolith interface, not a minimal subset.

The monolith's `DefaultResourceManager` has 7 methods: `can_afford`, `try_reserve`, `release`, `quota`, `spent_cost`, `spent_tokens`, `record_usage`. An initial implementation ported only 4 (dropping `try_reserve`, `release`, `record_usage`) and changed `can_afford`'s signature from `&ExecutionGraph` to `(f64, u64)`.

This was a scope error. Reasons to keep all 7:
- The trait represents the *full resource management interface*, not just the budget-check subset
- `try_reserve` and `release` are used by the executor (not the compiler pass), but they belong on the trait because the trait is the resource management contract
- `record_usage` is used by the stream meter
- At production cutover, the monolith's `DefaultResourceManager` will implement this trait directly — a 4-method trait can't be a drop-in replacement

The `can_afford` signature stays as `(f64, u64)` (not `&ExecutionGraph`) because the compiler pass only needs cost/tokens, and taking the full graph would couple the trait to the IR. The monolith's `DefaultResourceManager` will implement the trait method by extracting cost/tokens from its own logic, not from the graph.

### Q5: What does the stub's `can_afford()` actually check?

**Decision:** The stub tracks state and checks against quota (not a dumb `true`).

An initial implementation had `can_afford()` always return `true`, making accumulation tests impossible. This was fixed: the stub uses `AtomicU64` counters, and `can_afford()` checks whether `(current_cost + estimated_cost) <= max_cost && (current_tokens + estimated_tokens) <= max_tokens`.

This enables the accumulation test cases from Q3. The stub is still a simplified test-double — it doesn't handle reservation/release semantics, just raw accumulation — but it's honest about state.

## Consequences

- The trait becomes reusable by future crate-side code without coupling to the monolith's lifecycle
- The thin-pass-plus-stub pattern is the right default for simulation-only crates
- Equivalence tests for stateful passes are a new pattern — straightforward but distinct from the pure-function pattern used by prior passes
- The production cutover decision (wiring live state into the crate) is explicitly deferred, not accidentally defaulted into
- The 7-method trait is a 1:1 match with the monolith's interface — zero adaptation needed at cutover

## Scope

**In scope for this ADR:**
- `ResourceManager` trait (7 methods) + `Quota` → `fusion-kernel`
- `BudgetOptimisationPass` + `BudgetError` → `fusion-compiler`
- State-aware `StubResourceManager` with `AtomicU64` counters for crate tests
- Accumulation test cases (`budget_pass_accumulates_spend`, `budget_pass_shared_state`)

**Out of scope:**
- `DefaultResourceManager` stays in monolith — no change
- `CompilerEngine::new()` stays zero-arg — no change
- Wiring live state into the crate — deferred to production cutover
- Reservation/release logic in the stub (the stub accumulates cost but doesn't model reservation semantics)

---

> **Governance note:** `docs/adr/` (uppercase) contains duplicate ADR numbers (two ADR-001s, duplicates at 002–008). This is pre-existing governance debt, not introduced by this ADR. `docs/adrs/` (lowercase) is the canonical directory for ADRs ≥ 017. Future ADRs should use `docs/adrs/` and sequential numbering from 039 onward.
