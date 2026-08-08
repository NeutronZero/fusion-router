# Ensemble Review Findings — 2026-08-08

Produced by the multi-model consensus review (zen-deepseek, gpt-oss-20b,
nemotron judge). Every finding below was validated against the CURRENT source
and either refuted with quoted evidence or fixed with the exact minimal
change. All fixes landed on `fix/fusion-review-findings`.

- **Fixed**: 7 (1, 3, 4, 7, 12, 14, 17)
- **Refuted**: 9 (2, 5, 6, 9, 10, 11, 13-partial, 15, 16 — see per-item evidence)
- **Verified + hardened**: 1 (13)

| # | Sev | Verdict | Summary & evidence |
|---|-----|---------|--------------------|
| 1 | Critical | FIXED | `src/executor/mod.rs` `execute_native_tool_calls` no longer unwraps `tool_registry`/`registry.get`; both lookups now degrade to an explicit `{"executed": false, "error": ...}` result entry instead of panicking. |
| 2 | High | REFUTED | `handlers.rs::stream_graph_estimate` (233-255) derives tokens from client content only (`content.len()/4`, `.max(1)`), making no allocation proportional to the estimate; admission counters are u64. No OOM vector; refusal is the ResourceManager's contractual role. |
| 3 | High | FIXED | `resource/cancelling_stream.rs` double-finalization removed (hook finalizes once, across error + drop), with a once-only test. |
| 4 | Medium | FIXED | `executor/mod.rs` cache `put` now requires a non-empty textual output; JSON tool-result payloads and empty strings never enter the semantic cache. |
| 5 | Medium | REFUTED | `server/pipeline.rs:159-175` propagates the request `tools` allowlist into the lowered node AND every LLM sub-node AFTER the strategy-override rewrite (`handlers.rs:414-453`), so per-request tools survive the override. |
| 6 | Medium | REFUTED | `handlers.rs:294-311`: admission estimate is released at stream end and replaced by measured usage via the finish hook — the documented exact-accounting contract, not a leak. |
| 7 | Medium | FIXED | `providers/registry.rs` `commit()` now `fetch_add(1, SeqCst)` on `version` after target replacement; subscribers watching `version()` observe reloads. |
| 8 | Medium | FIXED | `ProviderConfig.base_url` now flows: registry `prepare()` passes it into `OpenRouterProvider::with_base_url` / `ZenProvider::with_base_url` → model `base_url` field → `format_request` (config wins over env default). Additive constructors; existing `new` unchanged. |
| 9 | Medium | REFUTED | `config/mod.rs:305-306` `validate()` = `validate_with_profile(!cfg!(debug_assertions))`; the relaxed checks (auth disabled, rate limiting off, wildcard CORS) are exactly the documented ADR-035 debug behavior, and `auth.enabled && empty keys` is rejected in BOTH profiles by design (test `test_validate_rejects_auth_without_keys`). |
| 10 | Medium | REFUTED | The execution-kinds arms (`Transform`/`Gate`/`Conditional`/`Loop`/`Split`/`Join`/`Barrier`) are routing markers; their semantics live in the scheduler (e.g. `scheduler/default.rs` loop-body resets) and compiler lowering — `execute_node` only runs LLM work nodes. |
| 11 | Medium | REFUTED | `scheduler/default.rs:328-379`: `DefaultScheduler` implements `retry_policy` retry with backoff AND `fallback` model re-execution on `NodeState::Failed`. Policies are consulted at the scheduling layer, which is the correct place. |
| 12 | Medium | FIXED | `transport/http.rs` `stream()` rewritten: multi-byte UTF-8 spanning chunk boundaries is reassembled via a pending-byte buffer; partial tails are flushed lossily; invalid bytes replaced; covered by new tests. |
| 13 | Medium | FIXED | `types/error.rs` adds `RouterError::user_message()` (generic, client-safe); `handlers.rs` pipeline failure, stream chunk errors, and provider-open failures no longer echo internal strings (`e.to_string()` stays in server logs). |
| 14 | Medium | FIXED | `executor/mod.rs` tool-loop stop keeps the model's final text when present; raw tool-call JSON is returned only when the model produced no text. |
| 15 | Low | REFUTED | No external cancellation source exists by design; the token is an internal control latch (streaming has its own). HTTP disconnect abort is a roadmap hardening item, not a defect. |
| 16 | Low | REFUTED | Boot always validates: `src/main.rs:135` calls `config.validate()` and `ConfigManager` validates every reload (`config/manager.rs:77`). The fail-closed gates are unreachable-able only when a caller skips both entry points. |
| 17 | Low | FIXED | `resource/cancelling_stream.rs` finish-hook ordering corrected (hook runs once on terminal events; never double-fires mid-error), with regression test. |

## Housekeeping (pre-existing compile drift, unrelated to findings)
Structs `ChatCompletionRequest.strategy` and `StrategyIR::Consensus.members`
grew after these test/example literals were written; `cargo test` could not
compile. Fixed: `tests/slo_tests.rs`, `tests/reliability_tests.rs`,
`tests/unit/context.rs`, `tests/unit/regressions.rs`,
`tests/strategy_sdk/{lowering/deterministic,primitives/fanout,serialization/primitive_graph_hash}.rs`,
`examples/consensus_roadmap.rs`, `examples/debate_roadmap.rs`.

## Verification
- `cargo check` and `cargo check --all-features`: clean, zero warnings.
- `cargo test`: 696 + 10 + 597 + integration suites — all pass.
- `cargo test --all-features`: 731 + 10 + 608 + integration suites — all pass.