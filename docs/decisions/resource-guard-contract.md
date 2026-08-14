# ResourceGuard Contract

- **Date**: July 2026
- **Status**: Clarification

## Context

`ResourceGuard` guards a reserved resource quota during request execution. Understanding the release contract is critical for correctness — bugs here mean leaked quotas or premature release.

## The Contract

`ResourceGuard` has **two complementary release paths** — they are not alternatives but a safety net over the primary path:

### Primary path: explicit `commit()`

- `commit()` sets `committed = true`, signaling successful execution.
- Resources remain reserved for the duration of the `ResourceGuard`'s lifetime.
- Used in `handlers.rs:354` only after all pipeline steps succeed and the response is built.

### Safety net: RAII `Drop`

- If `ResourceGuard` is dropped **without** `commit()` having been called, `Drop::drop()` calls `resource_manager.release(&graph)` to release the reserved quota.
- This is the **sole release mechanism on all error paths** — any `?` operator before `guard.commit()` causes the guard to go out of scope and release resources automatically.

### Rule

| Path | `committed` | On `Drop` | Intended for |
|------|-------------|-----------|--------------|
| Explicit `commit()` | `true` | No-op (quota retained) | Successful execution |
| Implicit `Drop` | `false` | Calls `release()` (quota freed) | Error / cancellation / early return |

**Both paths are relied upon.** The design intentionally uses RAII as the error-recovery mechanism — no manual `release()` call is needed on failure branches.

## Usage pattern (`handlers.rs`)

```rust
let mut guard = step_reserve.execute(graph, &mut pctx).await?;
//           ^^^— If this or any subsequent step fails,
//               the ? propagates and guard drops → release()

let result = step_exec.execute(...).await?;
let response = ResponseBuilderStep.execute(result, &mut pctx).await?;

guard.commit();   // Only after all steps succeed
```

## Caveats

1. **Async runtime requirement**: `Drop::drop()` uses `tokio::runtime::Handle::try_current()` to spawn the `release()` call. If no tokio runtime is active when the guard drops, the release is silently lost (`let _ = ...`). This is acceptable because `ResourceGuard` is always used within request-scoped tokio contexts.
2. **No double-release protection**: If `release()` is called and then `commit()` is later called on a separately-cloned guard, the committed flag applies to that clone only. Each clone tracks its own `committed` state because `committed: bool` is not `Arc`-shared.
3. **`release()` is best-effort**: The return value of `release()` is discarded. Failures in resource release are logged but do not propagate.
