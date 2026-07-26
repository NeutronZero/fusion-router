# Sprint 1.3 Task 1: `update_thresholds()` for CircuitBreaker

## Status: ✅ Complete

## Changes made to `src/providers/circuit_breaker.rs`

1. **Import**: Added `AtomicU64` to the atomic imports.

2. **Struct fields**:
   - `failure_threshold: u32` → `failure_threshold: AtomicU32`
   - `cooldown_duration: Duration` → `cooldown_secs: AtomicU64`

3. **Constructor** (`new`): Uses `AtomicU32::new(failure_threshold)` and `AtomicU64::new(cooldown_secs)`. Signature unchanged.

4. **`can_execute()`**: Reads cooldown via `self.cooldown_secs.load(Ordering::Relaxed)` and converts to `Duration`.

5. **`record_failure()`**: Reads threshold via `self.failure_threshold.load(Ordering::Relaxed)`.

6. **New method `update_thresholds()`**: Stores both values atomically.

## Verification
- `cargo check` — ✅ zero new errors/warnings
- `cargo check --lib` — ✅ clean
- `cargo check --no-default-features --lib` — ✅ clean
- Unit tests (`cargo test` with `circuit_breaker` filter) — blocked by pre-existing `AppConfig::connectors` error in unrelated files

## Commit
```
63f2e56 feat: make CircuitBreaker thresholds atomic, add update_thresholds()
```
