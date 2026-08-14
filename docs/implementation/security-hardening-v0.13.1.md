# v0.13.1 — Security Hardening Milestone Charter

**Status:** FROZEN (2026-08-03). Changes require review of the frozen contract; implementation must not redefine charter content while in progress — distinguish "the architecture changed" (charter amendment) from "the implementation drifted" (fix the code).
**Owner:** TBD.
**Goal:** Convert every documented architectural boundary into an enforced runtime invariant.
**Success criteria:** Fail-closed by default; policy can block; every execution path shares one compiler pipeline; sandbox permissions are per-plugin; all external boundaries have deadlines and bounded queues.

Source of truth for findings: security audit (2026-08). Line numbers verified against current `HEAD`.
Architectural decisions herein are recorded as **ADR-034, ADR-035, ADR-036, ADR-037** (Draft) and amend **ADR-012** (Security Model); see Section 0.3.

## Out of Scope

This is a hardening milestone. The following are explicitly **not** in scope and any proposed change in these areas during v0.13.1 is rejected unless it is required to close a finding in this charter:

- No new planner features (no new planner types, no planner behavior changes beyond WP 1.3 policy enforcement).
- No new routing/strategy features (no new strategies, no strategy behavior changes beyond Law 7 tool invocation semantics).
- No model capability expansion (no new providers, models, or provider feature additions except native `tool_calls` plumbing required by WP 3.1).
- No API-breaking feature additions except those explicitly required for security (structured tool invocation, plugin execution context, fail-closed config).
- No performance work beyond the findings listed in Phase 5.
- No refactors with no security/correctness effect; no style/formatting work.

---

## 0. Milestone Laws

Two families: **Compiler Laws** (1–5, the execution-compiler contract) and **Runtime Laws** (6–10, the deployment and execution contract). Each law is an executable invariant backed by a test that lands **before** the fix (TDD).

### 0.1 Compiler Laws

| # | Law | Enforcement point | Test |
|---|---|---|---|
| Law 1 | Every execution graph must pass all mandatory compiler passes | Single `build_compiler()` factory; `DefaultCompiler` construction restricted | `tests/security_invariants.rs::law1_*` |
| Law 2 | Every node must satisfy all applicable policies | `PolicyCompilerPass` rejects `Deny` matches; `apply_policy` on all capability resolution paths | `law2_deny_blocks_compilation` |
| Law 3 | Every capability must have an approved permission set | Registry freeze-time validation + per-plugin execution-time checks | `law3_*` |
| Law 4 | ExecutionGraph construction is impossible after compiler failure | `DefaultCompiler::compile` already fails on pass error — add negative test | `law4_compile_failure_yields_no_graph` |
| Law 5 | No execution endpoint may bypass `WorkflowCompiler` | `build_execution_plane` uses the shared factory | `law5_execution_plane_uses_full_passes` |

### 0.2 Runtime Laws

| # | Law | Enforcement point | Test |
|---|---|---|---|
| Law 6 | Release builds fail closed on insecure defaults | `validate()` rejects auth-off/rate-limit-off in release; `--unsafe-dev` is the only escape hatch | `law6_release_fails_closed` |
| Law 7 | Model output is never interpreted as executable actions | Executor consumes provider-native `tool_calls` only | `law7_no_freeform_tool_parsing` |
| Law 8 | Host functions enforce permissions of the invoking plugin only | `PluginExecutionContext` threaded into host; no registry scan | `law8_host_permission_is_callers` |
| Law 9 | Every external boundary has a deadline; every queue is bounded | Timeout/backpressure sweep | `law9_*` |
| Law 10 | Every externally supplied path is canonicalized and proven to remain inside its trust root before use | Central path-safety helper applied at registry, plugin loader, file tools, package extraction, attestation archive | `law10_path_containment` |

Law 10 is enforced once in `src/security/paths.rs` (new module: `canonicalize_within(root, candidate) -> Result<PathBuf>`), and reused by every consumer instead of scattered per-site checks. Baseline already exists for `FileReadTool` (`src/tools/builtin.rs:128-136`) — Law 10 extends that pattern to `FilesystemPackageRegistry`, plugin `entry` resolution, and package extraction.

### 0.3 ADR Register

The major architectural shifts are ADR-level decisions, not implementation details. Note: ADR-029–033 are taken (Execution Semantics, Session Replay, Trigger Semantics, Execution ABI, Architecture Freeze); new ADRs start at **034**. ADR-012 (Security Model) is amended, not replaced.

| ADR | Title | Drafted in | Covers |
|---|---|---|---|
| ADR-034 | Single Compiler Pipeline | Phase 1 (WP 1.1–1.3) | Laws 1, 2, 4, 5; `build_compiler()` as sole production construction path; policy denial as compile error |
| ADR-035 | Fail-Closed Deployment | Phase 2 (WP 2.1) | Law 6; `--unsafe-dev`; default bind/auth/rate-limit/CORS/tool posture |
| ADR-036 | Plugin Execution Context | Phase 4 (WS-A/B) | Law 8; `PluginExecutionContext` in `Store` data; caller-bound permissions; URL-structural matching |
| ADR-037 | Structured Tool Invocation | Phase 3 (WP 3.1) | Law 7; provider-native `tool_calls` only; per-request tool allowlist |
| ADR-012 (amendment) | Security Model | Milestone-wide | Fail-closed defaults, policy denial semantics, per-plugin trust boundary, Law 10 path contract |

ADRs land in `docs/adr/ADR-0NN-*.md` per repo convention; `.memory/adrs.md` index updated with each.

---

## Phase 1 — Compiler Contract Enforcement (Tier 0)

**Exit criteria:**
- Law 1, 2, 4, 5 tests pass.
- A `deny` rule causes compilation failure (integration-tested end to end).
- `POST /v1/executions` input violating constraints, budget, or policy is rejected (no execution occurs).
- No production construction path with an empty pass list remains (grep-in-CI: `DefaultCompiler { passes: vec![] }` forbidden outside `#[cfg(test)]`).

### WP 1.1 — Enforce `PolicyEffect::Deny` in the compiler pass

- **Findings:** C5 (+ M2 policy parsing).
- **Files:**
  - `src/compiler/passes/policy.rs` (`apply`, lines 43–82): on matched rule with `PolicyEffect::Deny`, return `Err(CompilerError::ValidationError { .. })` carrying `rule_id` and `target_pattern` **before** any Approval handling. Do not insert gate nodes for denied targets.
  - `src/policy/ir.rs` (`from_ast`, lines 49–53): unknown `effect` string → `Result<PolicyIR, PolicyError>` (fail closed). Accept only `deny`/`approval`/`allow`; reject case-mismatch and typos.
  - `src/policy/precedence.rs` (`evaluate_matching_rule`, lines 11–15): replace first-match `.find()` with collect-then-`max_by_key` on `(effect, priority)` so precedence holds for any `PolicyIR` regardless of input order or `Deserialize` source.
  - `src/policy/ir.rs` (line 38): add `#[serde(deny_unknown_fields)]` to `PolicyIR`; normalize via `from_ast` in a `try_from_ast` that surfaces parser diagnostics (`src/policy/ast.rs:34-41`).
- **Tests:**
  - `src/compiler/passes/policy.rs` unit: deny rule → `apply` returns Err; approval still inserts gate; deny-outranks-approval on same target.
  - `src/policy/ir.rs`: unknown effect string errors; unsorted input yields correct precedence (existing `test_precedence_evaluation_deny_over_approval` fed raw unsorted `PolicyIR`).
  - Law 2 test: `law2_deny_blocks_compilation`.
- **Memory/ADR:** `.memory/policies.md`, `.memory/compiler.md`, ADR-034, `docs/specifications/compiler-passes.md`.

### WP 1.2 — Single compiler pipeline; execution plane uses full passes

- **Findings:** C2 (+ associated budget/concurrency bypass).
- **Files:**
  - `src/compiler/mod.rs`: add `pub fn build_compiler(config) -> DefaultCompiler` (or `CompilerPipeline`) containing the full mandatory pass set — `ConstraintValidationPass`, `ControlFlowValidationPass`, `ModelResolutionPass`, `BudgetOptimisationPass` — plus the optimization module (`FanOutConsolidationPass`, etc.) and `PolicyCompilerPass` wiring point. Keep `DefaultCompiler { passes: vec![] }` behind `#[cfg(test)]` or remove entirely.
  - `src/server/execution.rs` (`build_execution_plane`, line 283): use `build_compiler(...)` instead of `DefaultCompiler { passes: vec![] }`.
  - `src/server/handlers.rs` (lines 92–104): refactor to consume the same factory (delete duplicated pass list).
  - `src/server/execution.rs` (`ExecutionPlane::execute`): add `ResourceManager::try_reserve` (mirror `src/server/pipeline.rs:147-171`), install `BudgetEnvelope`, enforce `max_concurrent` via semaphore, and validate node count/model allowlist before compile.
- **Tests:**
  - `tests/security_invariants.rs::law5_execution_plane_uses_full_passes` — assert compiled graph from `/v1/executions`-shaped input is rejected when it violates constraints/budget (e.g., dangling edge, over-budget node, deny-rule target).
  - Unit: `build_compiler` returns non-empty, ordered pass list.
  - Regression: existing `/v1/executions` tests (`tests/integration_tests.rs`) still pass with validation now active.
- **Memory/ADR:** `.memory/architecture.md`, `.memory/compiler.md`, ADR-034, `docs/specifications/compiler-passes.md`.

### WP 1.3 — Capability policy enforcement on all resolution paths

- **Findings:** H13.
- **Files:**
  - `src/planner/resolver/capability/resolver.rs`:
    - `apply_policy` (lines 237–253): extend signature to `(req, contract, target)` and reuse for all paths.
    - `expand_dependencies` (lines 320–322): call `apply_policy` for every contract added to `result_map` (incl. transitive), not just `required_capabilities` (lines 261–268).
    - Version-constrained (274–282) and optional (308–318) paths: same check before `visited.insert`.
    - Final belt-and-braces: after resolution, iterate `final_instances` and re-verify `deny_list`/`allow_list`; fail resolution on any violation.
- **Tests:** unit tests in `resolver.rs`: deny-listed capability via (a) version constraint, (b) optional requirement, (c) transitive dependency — all rejected; allow-listed transitive dependency passes.
- **Memory/ADR:** `.memory/capability-system.md`, `.memory/planner.md`, ADR-034.

---

## Phase 2 — Fail-Closed Deployment (Tier 1)

**Exit criteria:**
- A default install (no config, no flags) is unreachable without authentication, bound to `127.0.0.1`, rate-limited, CORS same-origin, with shell/HTTP tools disabled.
- `--unsafe-dev` is required for every insecure configuration (auth off, rate limit off, wildcard CORS, permissive tools); each trigger logs a prominent warning.
- `validate()` rejects insecure combinations in release builds.
- Rate limiter keys on unspoofable peer identity; API-key comparison is constant-time.

### WP 2.1 — Release-build fail-closed defaults

- **Findings:** C1 (+ C3/H1 tool defaults).
- **Files:**
  - `src/config/mod.rs`: default `host` → `127.0.0.1`; `AuthConfig::enabled` default → `true`; `default_rate_limiting_enabled()` → `true`; `default_cors_origins()` → `vec![]` (same-origin only); `default_allowed_shell_commands()` → `vec![]`; `default_enable_http_tool()` → `false`. Add `unsafe_dev: bool` to `AppConfig` (default false).
  - `config/default.yaml`: mirror the above (`auth.enabled: true` with `api_keys: []`, rate limiting on, tools lists empty).
  - `src/config/mod.rs` `validate()` (lines 325–332): in `!cfg!(debug_assertions)`, return error when `auth.enabled == false && !unsafe_dev`, or `rate_limiting.enabled == false && !unsafe_dev`, or `cors.allowed_origins` contains `*` without `unsafe_dev`.
  - `src/main.rs`: accept `--unsafe-dev` flag that sets `unsafe_dev = true` and logs a prominent warning; `resolve_api_key` placeholder (`main.rs:54-70`) gated to `unsafe_dev` (dev-only).
- **Tests:**
  - `src/config/mod.rs` unit: defaults assert fail-closed values; `validate` rejects insecure combos in release-mode helper (simulate via a `build_profile` parameter).
  - `tests/security_invariants.rs::law6_release_fails_closed`.
  - Integration: server refuses to boot with `--unsafe-dev` absent and auth disabled config.
- **Memory/ADR:** `.memory/architecture.md`, `docs/operator/*` config docs, `.memory/glossary.md` (`unsafe-dev`), ADR-035.

### WP 2.2 — Hardening of the auth/rate-limit path (medium items)

- **Findings:** M2 (rate-limit bypass), M3 (timing-safe key compare).
- **Files:**
  - `src/middleware/rate_limit.rs` (lines 40–62, 72–122): derive bucket key from `ConnectInfo` peer address (axum) — never raw `x-forwarded-for` unless a configured trusted-proxy list exists (then take rightmost untrusted hop); cap bucket count (e.g., 100k, evict LRU or deny); move limiter inside auth layer so it keys on authenticated identity when auth is on.
  - `src/middleware/auth.rs` (lines 44–50): constant-time comparison over SHA-256 digests (`subtle::ConstantTimeEq`); reject keys > 1 KB before hashing; add `x-api-key` length guard.
- **Tests:** spoofed `x-forwarded-for` cannot reset buckets; bucket cap enforced; timing-safe compare unit test; shared-`"unknown"`-bucket starvation mitigated.
- **Memory/ADR:** `.memory/architecture.md` middleware section, ADR-035.

---

## Phase 3 — Tool Execution Trust Boundary (Tier 2)

**Exit criteria:**
- Zero occurrences of free-form JSON tool parsing in the executor (grep-in-CI: no `serde_json::from_str` on model output feeding tool dispatch).
- Tool execution happens only via provider-native `tool_calls` with a per-request allowlist.
- Shell and HTTP tools are disabled by default; argument/URL policy tests pass.
- A prompt-injection chain cannot produce any tool execution.

### WP 3.1 — Remove free-form JSON tool execution

- **Findings:** H2 (chain root), supports C3/H1.
- **Files:**
  - `src/executor/mod.rs` (lines 242–270): delete `serde_json::from_str(model_output)` → tool dispatch. Replace with a structured `ToolCallRequest` path fed **only** from provider-native `tool_calls` (add a `native_tool_calls: Option<Vec<ToolCall>>` field to provider response models in `src/providers/*_model.rs` and thread through `src/executor/mod.rs`).
  - Add per-request `tools.allow_auto_exec` config flag + per-session tool allowlist (request-scoped).
  - If a provider lacks native tool support, tool execution for that provider is disabled (fail closed), not emulated.
- **Tests:** `law7_no_freeform_tool_parsing` (output containing tool JSON string is returned as text, never executed); provider with structured tool_calls executes only allowlisted tools; regression on strategy tests (`tests/strategy_sdk/*`).
- **Memory/ADR:** `.memory/execution.md`, `.memory/providers.md`, ADR-037, `docs/specifications/*` tool call contract.

### WP 3.2 — Shell tool argument policy; safe defaults

- **Findings:** C3.
- **Files:**
  - `src/tools/shell_tool.rs` (lines 41–74, 105–134): add per-command argument validation — for known file-reading commands enforce canonicalized path prefix containment via the Law 10 helper (`src/security/paths.rs`); add `allow_unrestricted_args` flag defaulting false; enforce `shell_timeout_secs` with `tokio::time::timeout`.
  - `src/config/mod.rs` (232–234): default allowlist empty.
- **Tests:** `cat ../secret` rejected; canonicalized symlink escape rejected; timeout enforced; existing injection tests (`shell_tool.rs:233-254`) extended.

### WP 3.3 — HTTP tool URL policy

- **Findings:** H1.
- **Files:**
  - `src/tools/http_tool.rs` (lines 63–107): scheme allowlist (https default), block loopback/link-local/private ranges with DNS resolve-then-recheck (mitigate rebinding), `redirect::Policy::none()` or re-validate each hop, cap response body (e.g., 1 MB), restrict settable headers (drop `Authorization`/`Host` overrides), timeout.
- **Tests:** metadata-IP fetch rejected; redirect to internal host rejected; oversized body truncated; allowlisted external URL works.

---

## Phase 4 — Plugin Trust Boundary (single subsystem project, Tier 3)

WP 4.1–4.5 from the original audit are one coherent subsystem — a single trust chain:

```
Plugin Identity  →  Permission Model  →  Sandbox  →  Attestation  →  Package Verification
      │                   │                 │              │                  │
  WS-A              WS-B               WS-C          WS-D                WS-E
```

Tracked as one project with five workstreams (WS-A…WS-E) sharing a common `PluginExecutionContext` concept and the Law 10 path helper. A WS cannot exit until its upstream WS is merged.

**Exit criteria:**
- Zero `registry.list().find(...)` permission scans remain in host services (grep-in-CI).
- A zero-permission plugin cannot read secrets or make HTTP calls, even if other contracts hold `Secrets(*)`/`Http(*)`.
- WASM guest execution is fuel-metered, memory-capped, and wall-clock timed; infinite-loop plugins trap or time out without pinning executor threads.
- Attestation signatures bind manifest + module digests; repackaged/malicious packages are rejected; the mock verifier is `#[cfg(test)]`-only.
- Law 10 conformance: registry, loader, and extraction all canonicalize within their trust roots; extraction is size-capped.

### WS-A — Plugin Identity (`PluginExecutionContext`)

- **Findings:** C4 (identity half), H10.
- **Files:**
  - `crates/fusion-plugin-api/src/lib.rs`: add `PluginExecutionContext { capability_id, package_hash: [u8;32], permissions: Vec<Permission>, fuel_budget: u64, memory_limit_bytes: u64 }`.
  - `crates/fusion-plugin-api/src/lib.rs` (`CapabilityId::new`): validate `[A-Za-z0-9._-]`, reject `/ \ ..` and NUL.
- **Tests:** identity carried through `Store` data; `CapabilityId` traversal inputs rejected at construction (`law10_path_containment` subset).

### WS-B — Permission Model (caller-bound)

- **Findings:** C4 (enforcement half), M18, M19.
- **Files:**
  - `src/runtime/wasmtime_host.rs` (lines 69–94): `fetch_secret`/`http_request` check `caller.permissions` only (via `Store` data); remove `registry.list().find(...)` pattern entirely.
  - `src/runtime/policy.rs` (`glob_match`, lines 36–44): URL-structural matching — parse URL, exact scheme/host/port (IDNA + trailing-dot normalized), `*` permitted in path only; bare `*` requires explicit operator flag.
  - `src/runtime/linker.rs` (stub returns): wire real host functions with context, or keep fail-closed.
  - `src/executor/capability_executor.rs` (lines 19–70): add pre-execution permission check against the instance's contract (defense in depth).
  - `src/package/verifier.rs` (65–75): enforce `manifest.permissions` as authoritative — reject packages whose capability contracts declare permissions outside the manifest-declared set; enforce at registration (`src/package/loader.rs:47-56`).
- **Tests:** zero-permission plugin calling `fetch_secret` fails even when another contract has `Secrets(*)`; `Http("https://api.example.com*")` does not match `api.example.com.evil.com`; `law8_host_permission_is_callers`.

### WS-C — Sandbox (metering, memory, time)

- **Findings:** H7, H12 (+ H12-memory/table caps).
- **Files:**
  - `src/plugin/wasm.rs` (lines 41–64, 166–171, 184–199): `Config::default()` + `consume_fuel(true)`; `store.set_fuel(per_call)`; `ResourceLimiter` (memory + table caps); run guest calls via `spawn_blocking` + `tokio::time::timeout`; cap `read_string` at 1 MiB; do not execute guest code during discovery (`read_name` deferred or guarded).
  - `src/runtime/wasmtime_runtime.rs` (65–108, 135–168): honor `config.timeout_ms` (`runtime/config.rs:7,16`) and `ctx.deadline` (`runtime/context.rs:9`); reset fuel per `invoke`; check module-declared static memory against `memory_limit_bytes` at instantiation; `table_growing` returns `Ok(false)` above cap; `Config::max_wasm_memory`.
  - `src/wasm/runtime.rs`: keep existing fuel metering as the reference.
- **Tests:** infinite-loop plugin traps (fuel) and times out (wall clock); 4 GiB static-memory module rejected; `read_string` over cap rejected; concurrent invocation does not exhaust worker pool.

### WS-D — Attestation & Package Verification (content-bound)

- **Findings:** H8, M16.
- **Files:**
  - `src/release/attestation.rs` (lines 24–29): add `manifest_sha256`, `module_wasm_sha256`, `package_name`, `package_version`.
  - `src/package/format.rs` (`extract_package`, 36–68): hash `manifest.toml` + `module.wasm` during extraction; cap per-entry size (`entry.take(MAX)`, `entry.size()` check), total decompressed bytes, entry count; skip-and-count unknown entries.
  - `src/package/verifier.rs` (41–55): verify digests against signed attestation before returning `VerifiedPackage`.
  - `src/main.rs:229`: wire `ArchivePackageVerifier` with `HmacSha256Signer` from `FUSION_SIGNING_KEY` (fail startup when unset, mirroring `src/bin/fusion.rs:315-321`); move `MockPackageVerifier` (`src/operations/mod.rs:174-183`) behind `#[cfg(test)]`.
- **Tests:** repackaged malicious wasm with valid attestation rejected; gzip bomb rejected; endpoint reports unsigned package as invalid; startup fails without signing key.

### WS-E — Path Containment & Cache Identity (Law 10)

- **Findings:** H9, H10 (registry half), H11, module-cache poisoning (M17).
- **Files:**
  - `src/security/paths.rs` (new): `canonicalize_within(root, candidate) -> Result<PathBuf>` — single Law 10 implementation.
  - `src/package/filesystem_registry.rs` (18–35): use the helper on every `store/load/contains`.
  - `src/plugin/manager.rs` (100–136) + `src/plugin/manifest.rs` (20–26, 60–89): canonicalize `dir` + `entry`, require containment; reject absolute/`..` entries; reject duplicate plugin names; parse `version` as semver; require manifest `name` == file name.
  - `src/runtime/module_cache.rs` (7–30) + `src/package/loader.rs` (38–45): cache key includes wasm SHA-256; insert only after successful registration; bound cache size (LRU/eviction).
- **Tests:** traversal IDs rejected; registry operations refuse escapes; shadowing duplicate manifest rejected; stale-cache conflict impossible (content-hash key).

---

## Phase 5 — Runtime Robustness (Tier 4)

**Exit criteria:**
- Every external boundary (HTTP, LLM, tool, plugin, scheduler, transport) has a configurable deadline (grep-in-CI: `tokio::time::timeout` coverage on all `execute_node`/`send()`/guest-call sites).
- No unbounded `tokio::spawn` per event remains; stress test (10k events, slow consumer) shows bounded RSS and no dropped durable events.
- Lock-order test proves bind/register cannot deadlock; `poll_next` contains no blocking locks.
- Telemetry snapshot latency is flat at 1M rows (aggregates, no full-table scan on request path).
- Retry storms eliminated: 429 backs off exponentially, permanent 4xx never retried.

### WP 5.1 — Deadlines at every boundary

- **Findings:** H6 (+ H5).
- **Files:**
  - `src/scheduler/default.rs` (128–200, 370): wrap `execute_node` in `tokio::time::timeout(node_timeout_ms)`; fold backoff sleep into the `select!` against `CancellationToken`; add scheduler-level overall deadline from request config.
  - `src/transport/stdio.rs` (21–63): long-lived child per transport; `timeout` on reads; stderr drain task; `kill()+wait()` in `Drop`.
  - `src/transport/http.rs` (110–128) + `src/transport/backoff.rs` (19–26): retry only 429/5xx/network; exponential backoff on 429 (no reset-before-next); jitter floor `base/2 + rand(base/2)`; honor `Retry-After`; request timeout.
  - `src/providers/*`: add per-request provider timeout (config-driven).
- **Tests:** hung provider → scheduler completes with node failure within deadline; stdio child flood/hang scenarios; retry storm regression tests (`tests/resilience.rs`).

### WP 5.2 — Bounded queues and backpressure

- **Findings:** H4 (+ M7 event loss, M5 session growth, M6 consumers).
- **Files:**
  - `src/events/projection.rs` (30–58): one consumer task per projection with bounded `mpsc(256)`; propagate lag, no `tokio::spawn` per event.
  - `src/events/bus.rs` (25–38): durable consumers get their own bounded channel with `try_send` + backpressure; publishing with no durable subscriber must not fail the producer (decouple publish from delivery).
  - `src/events/consumers/storage.rs` (47–69): buffered writer per execution; batch flush.
  - `src/events/consumers/checkpoint.rs`, `timeline.rs`: watermark eviction for `saved_sequence_numbers`; cap timeline entries.
  - `src/session/store/memory.rs` (45–52): cap checkpoints per session (keep-last-N / coalesce).
- **Tests:** slow consumer does not drop checkpoints (bounded channel instead of broadcast); producer unaffected by consumer failure; session memory bounded.

### WP 5.3 — Concurrency and lock correctness

- **Findings:** M1 (lock inversion), M8 (blocking Mutex in poll), M9 (Drop clone/leak), M12 (hardcoded 16), M10 (label cardinality).
- **Files:**
  - `src/scheduler/connector_resolver.rs` (62–79): single global lock order (connectors → capability_map) or one combined map.
  - `src/resource/cancelling_stream.rs` (56–68): atomics/`tokio::sync::Mutex` for meter.
  - `src/resource/guard.rs` (32–48): store `Arc<ExecutionGraph>`; release via persistent background task + `watch` channel (no `Handle::try_current` skip).
  - `src/scheduler/work_queue.rs` (92–98): thread configured `max_concurrent`; indexed ready set; borrow instead of clone edges (`default.rs:252-255,324-327`).
  - `src/telemetry/metrics.rs` (79–83): bucket graph-hash labels (top-N + `other`).
- **Tests:** concurrent bind/register stress (no deadlock); quota released after drop outside runtime; queue respects configured concurrency.

### WP 5.4 — Telemetry writer and aggregate counters

- **Findings:** H3 (+ M11).
- **Files:**
  - `src/telemetry/sqlite_repo.rs`: incremental in-memory aggregates (counters per model/intent updated in `record()`); `snapshot()` reads aggregates; `get_model_stats` uses index + read-only connection; add indexes `(timestamp)`, `(model, intent)`; single-writer actor with bounded mpsc + batched prepared inserts; prune or partition table.
  - `src/telemetry/calibration.rs` (56, 133–137): read path against read replica/connection.
- **Tests:** snapshot latency independent of row count; insert throughput under concurrency; calibration no longer blocks writes.

### WP 5.5 — Idempotency, timestamps, and config atomicity

- **Findings:** M13 (webhook), M14 (fake timestamps — **minimal scope, see Deferrals**), M15 (config reload), M24 (checkpoint resume), M20 (intent constraints).
- **Files:**
  - `src/trigger/webhook.rs` + `src/trigger/engine.rs` (29–49): HMAC-SHA256 per-trigger secret verification; freshness window (timestamp) + nonce/idempotency store keyed `(trigger_name, body_hash)` with TTL; record `TriggerEvent::Deduplicated`; honor `X-GitHub-Delivery`-style headers when present.
  - Hardcoded timestamps: `src/lifecycle/manager.rs:30`, `src/session/checkpoint.rs:26`, `src/trigger/cron.rs:18`, `src/telemetry/unified_diagnostics.rs:37` → replace constant `1000` with `SystemTime::now()` (monotonic correctness only; no abstraction).
  - `src/config/manager.rs` (59–104): serialize reloads under a mutex; two-phase prepare/rollback; bump generation only on successful swap; poison recovery (`into_inner`) instead of `.expect`.
  - `src/session/checkpoint.rs` (19–27, 56–61): persist `current_node_id` + retry counters in `SessionSnapshot`; `resume_session` returns position; emit `RetryScheduled` (`types/execution_context.rs:33`).
  - `src/intent/lowering.rs` (8–13): emit `max_cost_usd`/`min_confidence` into node config.
- **Tests:** webhook replay rejected; duplicate delivery executes once; timestamps monotonic; concurrent reload leaves consistent generation; resume continues at last node; intent constraints round-trip.

---

## Phase 6 — Law Test Scaffold

**Exit criteria:**
- Laws 1–10 all green under `cargo test --all-features`.
- `cargo check --no-default-features --lib` clean.
- `python scripts/check-memory.py` passes (all `.memory/` updates in).

**Files:**
- `tests/security_invariants.rs` — one module per law (Laws 1–10), asserting the enforcement points above.
- `src/compiler/mod.rs`: `build_compiler()` marked as the sole construction path for production wiring; `DefaultCompiler` struct fields `#[doc(hidden)]` or test-only constructor.
- CI greps (or unit assertions) for the banned patterns: empty pass list, free-form tool parsing, `registry.list()` in host services, unbounded `tokio::spawn` in projection dispatch.

---

## Scope Deferrals (v0.13.2)

| Item | Rationale |
|---|---|
| M14 — injectable `Clock` trait | Excellent engineering, not a hardening requirement; v0.13.1 ships only the constant-replacement (monotonic real timestamps). Introduce the trait in v0.13.2 alongside deterministic replay work if replay is blocked on it. |
| M25 / remaining low-severity L-series items | Cosmetic; queue as backlog. |
| Replay `Deterministic`/`Simulation` implementations | Functional gap, not security; belongs with replay semantics work (ADR-030). |

---

## Sequencing & Dependencies

```
Phase 1 (compiler contract) ──▶ Phase 3 (tool trust) ──▶ Phase 4 (plugin trust boundary)
   └────────────┬───────────────┘        └──────────┬──────────┘
Phase 2 (fail-closed) ──────────────▶ Phase 5 (robustness) ─── all feed Phase 6 (laws)
```

- Phases 1 and 2 are independent and land first (highest leverage, largest attack-surface kill, lowest risk).
- Phase 3 depends on provider model changes (native `tool_calls`); verify against all three providers (`openrouter`, `zen`, `ollama`).
- Phase 4 workstreams are strictly sequential (WS-A → WS-B → WS-C → WS-D → WS-E); the project stays behind the `wasm-plugins` feature flag (default off) until WS-B lands.
- Phase 5 can start in parallel after Phase 2.
- Phase 6 tests land per-WP (TDD), not as a batch at the end.

## Risk Notes

- Removing free-form tool parsing (WP 3.1) changes runtime behavior for providers without native tool calls — ship as a behavior flag (`executor.allow_model_json_tools: false` default) for one release cycle, then delete.
- Tightening defaults (Phase 2) breaks existing dev workflows — that is the point; `--unsafe-dev` preserves them explicitly.
- Policy deny (WP 1.1) may reject currently-shipped default workflows if any match deny rules — audit `workflows/` and `config/default.yaml` during implementation.
- ADR numbering: 034–037 confirmed free; 029–033 already assigned (Execution Semantics, Session Replay, Trigger Semantics, Execution ABI, Architecture Freeze).
