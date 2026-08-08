# Self-Review Findings — 2026-08-08

Produced by codebase self-review pass over core hot spots (`src/transport/http.rs`, `src/executor/mod.rs`, `src/server/handlers.rs`, `src/providers/registry.rs`, `src/config/mod.rs`, `src/resource/cancelling_stream.rs`). Every finding was validated against current source and either refuted with quoted evidence or remediated with a minimal fix on branch `fix/self-review-2026-08-08`.

- **Fixed**: 2 (#1, #2)
- **Refuted**: 4 (#3, #4, #5, #6 — see per-item evidence)

## Findings Verdict Table

| # | Sev | Verdict | Summary & Evidence |
|---|-----|---------|--------------------|
| 1 | Medium | FIXED | `src/transport/http.rs` `drain_utf8`: when `from_utf8` returns `Err(e)` with `e.error_len() == None` (incomplete trailing sequence at chunk boundary), `drain_utf8` now drains and yields `carry[..valid_up_to()]` immediately if `valid > 0` instead of returning `String::new()`. Eliminates SSE streaming latency stutter. |
| 2 | Low | FIXED | `src/executor/mod.rs` `execute_node`: on LLM sub-node failure during strategy execution, `NodeExecutionResult` now returns `usage: accumulated_usage` instead of `None`, preserving token usage consumed by prior successful sub-nodes in the strategy. |
| 3 | Low | REFUTED | `src/server/handlers.rs:233-255`: streaming admission estimate `stream_graph_estimate` uses static token fallbacks. **Refutation**: Exact resource accounting is enforced post-stream by `metered_stream_with_finish` (302-308), which records actual usage (`report.total_tokens`, `report.cost_millicosts`) via finish hook when streaming completes. |
| 4 | Low | REFUTED | `src/providers/registry.rs`: `commit()` recreates target circuit breakers during config reload without preserving old state. **Refutation**: Re-instantiating `ProviderTarget` (221-225, 305) on config reload is the documented ADR-035 design intent to apply new threshold/cooldown configs dynamically. |
| 5 | Low | REFUTED | `src/config/mod.rs`: `RateLimitingConfig` `burst_size` zero value could cause divide-by-zero panics. **Refutation**: `AppConfig::validate()` (260-264) rejects `burst_size == 0` during startup/reload, ensuring zero values never reach middleware. |
| 6 | Low | REFUTED | `src/resource/cancelling_stream.rs`: dropping stream triggers double finish hook firing. **Refutation**: `fire_finish_hook()` (94-110) uses `self.on_finish.take()`, guaranteeing single execution across completion, error, or drop. Covered by unit test `test_metered_stream_finish_hook_runs_once`. |

## Verification Suite Results
- `cargo check`: CLEAN (0 warnings)
- `cargo check --all-features`: CLEAN (0 warnings)
- `cargo test`: PASS (698 passed)
- `cargo test --all-features`: PASS (733 passed)
- `python scripts/check-memory.py`: ALL CHECKS PASSED
