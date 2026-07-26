# Task 3 Report: Wire ProviderRegistry as ConfigSubscriber

## What I Implemented

1. **Removed `#[allow(dead_code)]`** from `CircuitBreakingProvider` struct — it's now a public type.
2. **Added `candidates` field** to `ProviderRegistry`:
   - `candidates: parking_lot::RwLock<Option<HashMap<String, Arc<ProviderTarget>>>>`
   - Initialized as `parking_lot::RwLock::new(None)` in `new()`
3. **Implemented `ConfigSubscriber` for `ProviderRegistry`** with two-phase lifecycle:
   - **`prepare()`**: Iterates `new.config.providers`, reads API keys from env vars, creates `ProviderTarget` instances with circuit breakers and factory closures, stores them in `self.candidates`
   - **`commit()`**: Takes candidates, computes added/removed/updated diff, logs it via `tracing::info!`, then atomically swaps `self.targets`
   - **`priority()`**: Returns `10` (runs after lower-priority subscribers)

## Key Decisions

- **`api_key_env` is `Option<String>`**: The brief's sample code treated it as `String`, but the actual `ProviderConfig` has `api_key_env: Option<String>`. I handle this by:
  1. Checking if the field is `Some` (error if `None`)
  2. Then reading the env var (error if missing)
- **No double-wrapping with `CircuitBreakingProvider`**: Per the architectural note, `ProviderTarget` already has its own circuit breaker, so the factory closures use `ProviderTarget::new` directly.
- **`CircuitBreakingProvider` module kept public**: Made the `#[allow(dead_code)]` removal, module already `pub mod` in `mod.rs`.

## Test Results

- `cargo check`: Zero warnings, zero errors
- `cargo test`: **All 207 tests pass** (unit), plus integration, golden, load, security, strategy SDK, and doc tests

## Files Changed

| File | Change |
|------|--------|
| `src/providers/circuit_breaking_provider.rs:8` | Removed `#[allow(dead_code)]` |
| `src/providers/registry.rs:6-7` | Added imports for `ReloadError`, `ConfigSubscriber`, `ConfigSnapshot` |
| `src/providers/registry.rs:18` | Added `candidates` field to struct |
| `src/providers/registry.rs:31` | Initialized `candidates: RwLock::new(None)` |
| `src/providers/registry.rs:137-216` | Added `impl ConfigSubscriber for ProviderRegistry` |

## Self-Review

- `ProviderRegistry` is `Send + Sync` (all fields are)
- Factory closures capture `name.clone()` and `api_key.clone()` — no lifetime issues
- `CircuitBreaker::new(failure_threshold, 3, cooldown_secs)` uses `success_threshold=3` as the default
- No pre-existing warnings were introduced; only pre-existing dead code warnings remain in other modules
- The `commit()` logs diff metrics before swapping — useful for observability

## Concerns

---

## Fix: Clean stale side maps on ProviderRegistry commit

**Problem:** `ProviderRegistry::commit()` swapped `self.targets` but left `self.prefixes`, `self.capabilities`, and `self.pricing` side maps stale. Removed providers could still be returned via `get_matching_targets()`.

**Fix applied in `src/providers/registry.rs:222-232`:**
1. Iterates `removed` names and removes corresponding entries from `self.capabilities`
2. Iterates `removed` names and removes corresponding entries from `self.pricing`
3. Clears `self.prefixes` entirely (config-driven reload doesn't provide prefix info; `get_matching_targets()` falls back to `default_target` on empty results)

**Test results:**
- `cargo check`: Zero warnings, zero errors

**Commit:**
- `446114d` fix: clean stale side maps on ProviderRegistry commit
