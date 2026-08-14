# ADR-039: Explain Route Real Scoring

- **Status:** Accepted
- **Date:** 2026-08-09
- **Applies to:** `crates/fusion-compiler` (explain_route, compile, route_scores, provider_comparison)
- **Depends on:** ADR-034 (Single Compiler Pipeline), ADR-038 (BudgetOptimisationPass Port)

## Context

`crates/fusion-compiler/src/lib.rs` contains three blocks of hardcoded, request-independent data:

1. `explain_route(provider_name)` — a match table returning fixed scores per provider name.
2. `compile()`'s `route_scores` — always calls `explain_route` for exactly `["openrouter", "zen", "ollama"]`.
3. `compile()`'s `provider_comparison` — a literal `Vec` with fabricated model names, scores, and prose reasons.
4. `compilation_time_ms: 2` — a hardcoded constant (fixed separately, real measurement now used).

The hardcoded scores produce plausible-looking output that doesn't vary with input. Benchmarking against these would produce numbers that look real but aren't.

This ADR documents the investigation of each sub-score's data source and the decision for how to handle it.

## Investigation Summary

### Frozen paths (from `scripts/check_monolith_freeze.py`)

- `src/compiler/` — **frozen** (read-only reference)
- `src/planner/` — **frozen** (read-only reference)
- `src/resource/` — **frozen** (read-only reference)
- `src/scheduler/` — **not frozen** (editable)

### Per-sub-score analysis

#### 1. `capability_score`

**Question:** Does `CapabilityResolutionPass` in `crates/fusion-compiler` do real capability resolution?

**Finding:** No. The pass is a no-op — `transform()` returns `ir.clone()` unchanged (line 166-168 of `crates/fusion-compiler/src/lib.rs`). The real capability resolution lives in `src/planner/resolver/capability/` which is **frozen**.

**Data source:** None in `crates/`. The pass discards no output because it produces no output.

**Decision:** Return `None` (not-yet-computed) rather than a fabricated number. Wire capability_score to `Option<f64>` — `None` means "no live capability resolution data available in crates/".

**Rationale:** A missing score is honest. A fixed 0.95 masquerading as a computed value is deceptive. Callers who need a score must source it from the monolith's planner at production cutover.

#### 2. `budget_score`

**Question:** Does `BudgetOptimisationPass` produce a usable budget score?

**Finding:** The pass works correctly — it calls `ResourceManager::can_afford()` and propagates the result. But it's backed by `StubResourceManager` with `f64::INFINITY` / `u64::MAX` quota (per ADR-038's deliberate scope decision). The stub always returns `true`.

**Data source:** `StubResourceManager` — always permissive by design.

**Decision:** Return `Some(1.0)` (always affordable) with a documented comment explaining why. This is correct given ADR-038's scope — the stub is intentionally permissive. Wiring a live `ResourceManager` is explicitly out of scope (production cutover territory).

**Rationale:** The score is computed from real data (the stub's state), not fabricated. It just happens that the stub's data always says "affordable." That's a correct reflection of the current configuration, not a bug.

#### 3. `latency_score`

**Question:** Does `ConnectorHealthChecker` in `src/scheduler/connector_health.rs` provide real latency data?

**Finding:** The `ConnectorHealthChecker` exists and is a real implementation with `health_map: Arc<RwLock<HashMap<String, ConnectorHealth>>>`. It's instantiated in `src/main.rs` (line 268). However, `check_connector_health()` always returns `latency_ms: 0` and `status: Healthy` — it calls `connector.descriptor()` but doesn't perform an actual network probe. The `run()` loop exists but connectors are never registered in practice for the crate compiler path.

**Data source:** `ConnectorHealthChecker` exists but is unpopulated for the crate compiler path. No live latency measurements flow into `crates/`.

**Decision:** Return `None` — no live latency data available in `crates/`.

**Rationale:** The health checker structure is real, but the data it would produce isn't available to `crates/fusion-compiler`. Fabricating a latency score would be worse than admitting the data isn't there.

#### 4. `health_score`

**Question:** Same as latency — does the health checker provide real health data?

**Finding:** Same finding. `ConnectorHealthChecker` always returns `Healthy`. The health map is populated only by the `run()` loop which requires registered connectors, which the crate compiler doesn't have.

**Data source:** None in `crates/`.

**Decision:** Return `None` — no live health data available in `crates/`.

**Rationale:** Same as latency_score.

#### 5. `policy_score`

**Question:** Does `PolicyCompilerPass` in `src/compiler/passes/policy.rs` provide real policy data?

**Finding:** `PolicyCompilerPass` contains real policy-matching logic — it lowers `PolicyIR` into `Gate` nodes via `PolicyPrecedenceEngine`. However, it's in `src/compiler/` which is **frozen**. The crate compiler (`crates/fusion-compiler`) has no policy pass at all.

**Data source:** Real logic exists in frozen monolith. Not portable to `crates/` without its own `PolicyIR`/`PolicyPrecedenceEngine` port (significant work, comparable to CapabilityResolver porting).

**Decision:** Return `None` — policy scoring is not wired in `crates/`. Porting policy scoring is a separate task (similar scope to ADR-038's capability resolver port).

**Rationale:** A `None` with a clear comment is better than a fixed `1.00` that looks computed. The policy scoring infrastructure exists in the monolith and can be ported later.

## Design Decisions

### D1: `ExplainRouteScore` fields become `Option<f64>`

```rust
pub struct ExplainRouteScore {
    pub provider_name: String,
    pub capability_score: Option<f64>,  // None = not yet computed
    pub budget_score: Option<f64>,      // Some(1.0) from stub, documented
    pub latency_score: Option<f64>,     // None = no live data
    pub health_score: Option<f64>,      // None = no live data
    pub policy_score: Option<f64>,      // None = not wired in crates/
    pub total_score: f64,               // Computed from available scores only
}
```

**`total_score` computation:** Only include scores that are `Some(...)`. Missing scores are excluded from the weighted average, and the weights are re-normalized proportionally. If all scores are `None`, `total_score` is `0.0`.

### D2: `route_scores` evaluated per IR, not fixed provider list

Instead of always evaluating `["openrouter", "zen", "ollama"]`, extract the set of relevant providers from the IR's node capabilities. Since capability resolution isn't wired yet (D1), fall back to the current fixed list but document it as a temporary fallback, not the intended behavior.

### D3: `provider_comparison` generated from computed deltas

Replace the literal prose with dynamically generated `reason` text based on score differences. When scores are `None`, the reason text says "Score not computed" rather than fabricating explanations.

### D4: `compilation_time_ms` already fixed

Real measurement using `std::time::Instant::now()` / `.elapsed()` around the pass loop. Committed separately.

## Consequences

- Callers see `None` for sub-scores that aren't wired, making it clear what's computed vs. missing
- `total_score` is honest — it only includes available data
- No fabricated numbers that could mislead benchmarks or diagnostics
- Future wiring of real data sources (capability resolver, health checker, policy engine) can replace `None` with `Some(value)` incrementally
- The ADR explicitly documents what's deferred and why, preventing future contributors from treating `None` as a bug to fix locally

## Scope

**In scope for this ADR:**
- `compilation_time_ms` real measurement
- `ExplainRouteScore` fields → `Option<f64>`
- `total_score` recomputation from available scores
- `provider_comparison` dynamic reason generation
- Tests proving scores vary with input (for `budget_score` which is the only `Some` score)

**Out of scope:**
- Wiring live `ResourceManager` into `crates/` (ADR-038 deferred to production cutover)
- Porting `CapabilityResolver` into `crates/` (separate ADR)
- Porting `PolicyPrecedenceEngine` into `crates/` (separate ADR)
- Wiring `ConnectorHealthChecker` data into `crates/` (requires live connector registration)
- Changing the frozen monolith files
