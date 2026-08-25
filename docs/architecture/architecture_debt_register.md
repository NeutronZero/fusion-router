# FusionRouter Architecture Debt Register

> **Purpose**: Tracks explicit architectural trade-offs, deferred capabilities, and planned structural refinements. This prevents temporary implementation choices from accumulating into silent technical debt.

---

## Active Architecture Debt Matrix

| ID | Area | Trade-off / Deferred Scope | Impact | Planned Resolution Target | Status |
|---|---|---|---|---|---|
| **AD-001** | Plugin Loading | In-process Rust C-ABI (`libloading`), WASM (`wasmtime`), and static plugins supported in v0.10. Out-of-process gRPC/IPC plugins deferred. | Third-party plugins in non-WASM languages require compiling against Rust ABI. | v0.11.0 | Planned |
| **AD-002** | WASM Permissions | Coarse-grained WASM sandbox fuel and memory limits in v0.10. Fine-grained capability-based syscall permissions deferred. | WASM plugins operate with coarse sandbox envelopes. | v0.11.0 | Planned |
| **AD-003** | Connector Resolver | Late binding binds single active connector per capability. Connector load balancing and dynamic failover deferred. | Single active connector binding per execution instance. | Future (v0.11.0 / v1.0.0) | Under Review |
| **AD-004** | Capability Cache | In-memory single-node LRU `CapabilityPlannerCache` for `RequirementSet` → `ResolvedCapabilitySet`. Distributed cache deferred. | Cache instances are local to each engine node. | Future (v1.0.0) | Under Review |
| **AD-005** | Compiler Optimization | `OptimizationPipeline` (dead-node elimination, fan-out consolidation, budget gate) is fully implemented and golden-tested but instantiated only in `#[cfg(test)]` (`src/compiler/optimization/mod.rs:291,299,310`). Never invoked in the server (`src/server/pipeline.rs`) or executor (`src/executor/mod.rs`) production flow. | Graph optimization is dormant: no dead-node pruning, no fan-out consolidation, no compile-time budget enforcement in production. | Behind `compiler.optimization_level` config flag | Planned |
| **AD-006** | Determinism Gate | `RealDeterminismBackend::compile_fixture` hard-codes `GateError::ToolNotAvailable` (`src/release/gates/determinism.rs:26-28`); gate is `required: false`. ReplayGate checks structural envelope only (`schema_version <= 1`, `format_version == 1`, payload non-empty) — never deserializes or hash-compares payloads. | No runtime verification that identical inputs produce identical execution graphs. | Wire real hash comparison when full compiler integration lands | Resolved (2026-08-26): DeterminismGate compiles every fixture under tests/fixtures/determinism twice and compares hashes; empty fixture sets fail closed; gate is required. ReplayGate parses v1 payloads as JSON and verifies declared payload_hash (sha256). Replay re-execution tracked as AD-019. |
| **AD-007** | Graph Hash Telemetry | `primitive_graph_hash` is set only by `to_execution_graph()` (`src/compiler/ir/primitive_ir.rs:276`), invoked only from executor strategy resolution. All other construction sites hardcode `0` (`src/server/handlers.rs:257`, `src/planner/resolver/capability/lowerer.rs:42,117`, `src/compiler/mod.rs:101`). `graph_hash_count` metric buckets 100% of production traffic into the all-zeros bucket. | Attestation/telemetry cannot distinguish graph identities; replay identity is unverifiable. | Propagate real hash through `CompilationStep` | Resolved (2026-08-26): production lowering sets primitive_graph_hash from compute_workflow_content_hash; remaining zero sites are test-only fixtures. Semantic note: the field carries a WorkflowIR content hash; PrimitiveGraph::compute_hash remains distinct (see review C5). |
| **AD-008** | Entry Node Invariant | `DeadNodeEliminationPass` treats `graph.nodes.first()` as the sole BFS root (insertion-order dependent). `to_execution_graph()` silently drops `(true, _)` edges out of `FanOut`/`Barrier` nodes (primitive_ir.rs:227-261) without diagnostics; orphaned successors become zero-in-degree roots that `WorkQueue` schedules immediately in parallel. `Barrier` semantics (`min_completion`, `timeout`, `on_failure`) have no `ExecutionNodeKind` and die at conversion. | Multi-root or mis-ordered graphs can lose entire subtrees or race parallel entries. Note: topo-sort-on-insertion is NOT a viable fix — feedback loops are acyclic-violating by design (`test_keeps_feedback_loop`). | Add explicit `entry_node_id` field on `PrimitiveGraph` | Resolved (2026-08-26): entry_node_id added (serde-defaulted); to_execution_graph now returns Result and errors on dangling edges / invalid entry instead of silently dropping; DeadNodeEliminationPass roots at the declared entry. Silent-drop hazard inside fusion-compiler strategy expansion remains open as AD-018. |
| **AD-009** | Release Gate Integrity | `PolicyEvaluator::evaluate` silently drops `GateExecution::ExecutionError` via `filter_map` (`src/release/evaluator.rs:94-97`). A required gate that errors (e.g. Determinism's `ToolNotAvailable`) is excluded from evidence entirely and never blocks. | Required gates can pass by omission; an Approve decision can rest on empty evidence. | Treat `ExecutionError` as a failed result; require non-empty evidence for Approve | Resolved (2026-08-02) |
| **AD-010** | Gate Backends Fabricate Evidence | `FilesystemStrategyBackend::load` returns a hardcoded passing artifact without reading the file (`gates/strategy.rs:109-120`); `ReplayGate::load_snapshot` stamps fabricated `0.10.0/format 1/schema 1` on every file (`gates/replay.rs:64-76`); `MockPackageVerifier` wired into prod attestation endpoints (`operations/mod.rs:111-120`, `main.rs:217`); `MockSigner` (DefaultHasher) signs attestations (`release/signing.rs:30-77`). | Certification, replay, and attestation verify fixtures, not artifacts — garbage passes; signatures are cryptographically meaningless. | Implement real backends that read file content and verify cryptographically | Resolved (2026-08-02) |
| **AD-011** | Strategy Dataflow | Subgraph `edges` are built by `execution_graph_to_subgraph` but never consumed; the executor iterates nodes linearly (`src/executor/mod.rs:133-257`). `Transform/Gate/Conditional/Loop/Split/Join/Barrier` are `{}` no-ops (`:249-255`); consensus/debate/fusion judges see only the LAST member's output (`:255`); strategy lowering failure silently falls to single-node passthrough with no log (`:284`). | Multi-agent semantics (the core value proposition) are functionally absent; broken strategies silently degrade to one plain LLM call. | Execute subgraph edges and accumulate member outputs into judge/collector context | Resolved (2026-08-02) |
| **AD-012** | Event/Replay Pipeline Unwired | `BroadcastEventBus` has zero production subscribers; `publish()` returns `Ok` while every envelope is dropped (`events/bus.rs:33`). Checkpoint file writes fail silently (`events/consumers/checkpoint.rs:58-61`); projection listener dies silently on `Lagged`/`Closed` (`events/projection.rs:31`); trigger/session/events modules unreachable from `main.rs` (blanket `allow(dead_code)` at `main.rs:1`). | ADR-026/030 replay, resume, and checkpoint claims rest on unexecuted code; all runtime events are lost. | Wire bus into server runtime or feature-gate the subsystem | Resolved (2026-08-02) |
| **AD-013** | WASM Memory Bounds | Guest-controlled `out_len` drives `vec![0u8; out_len as usize]` before any bounds check (`runtime/wasmtime_runtime.rs:154`); `read_string` slices `data[ptr..]` unbounded (`plugin/wasm.rs:184-193`). | Malicious/corrupt guest forces ~4 GB host allocation (OOM DoS) or a host panic. | Cap allocation to `memory.size()` before read; bounds-check pointer + len | Resolved (2026-08-01) |
| **AD-014** | Hardcoded Resource Limits | Iteration cap `10` silently truncates deep graphs mid-execution (`server/pipeline.rs:163-165`); `max_concurrent_nodes` hardcoded `16` ignores config (`scheduler/work_queue.rs:94`); budget commits spend before the limit check (`resource/budget.rs:26-44`); shell timeout drops the future but not the process (`tools/shell_tool.rs:65-70`). | Deep workflows fail with no diagnostic; over-charged spend on failed calls is permanent; timed-out commands keep running (leak/side effects). | Config-driven limits; reserve-before-commit with rollback; `kill_on_drop` | Partially Resolved (2026-08-26): record_and_check now rolls back both counters on violation (no permanent overspend); shell child output capture bounded to MAX_OUTPUT_BYTES during read. kill_on_drop landed 2026-08-24. Iteration caps config-overridable. Open: true reserve-before-call estimation. |
| **AD-015** | Telemetry Dead Counters | `request_duration_seconds`, `errors_total`, `tokens_total`, `provider_latency_seconds` registered but never incremented (`telemetry/metrics.rs:51-68`); `requests_total` only from the WASM host (`runtime/wasmtime_host.rs:96-99`); `graph_hash_count` uses the full 64-bit hash as a label (`server/handlers.rs:362-366`); audit JSONL emits an empty line on serialize failure (`telemetry/audit.rs:43`). | Latency/error/token visibility is zero for all traffic; unbounded Prometheus label cardinality; corrupted audit trail. | Wire counters into request lifecycle; bucket hash labels; fail the JSONL write | Partially Resolved (2026-08-26): POST /v1/executions now increments requests_total/errors_total and observes latency (previously uninstrumented); chat path live since earlier pass; graph_hash_count unlabeled. Open: tokens_total on executions plane, JSONL write failure behavior. |
| **AD-016** | Config Parsed But Ignored | `policies` are passed to `IntentPlanner::plan` which ignores them (`planner/intent_planner.rs:223`); `features`, `connectors`, `providers[*].base_url`, `shutdown_timeout_secs`, `logging.directory` are parsed (`config/mod.rs`) but never read; `FeatureRegistry` flags gate nothing at runtime. | Policy enforcement is a production no-op; operator configuration is silently inert. | Wire policy compilation into the compile path; delete or implement dead fields | Partially Resolved (2026-08-26): policy enforcement wired end-to-end (403 PolicyDenied) in the 2026-08-24 pass; providers.base_url/shutdown_timeout_secs/connectors now read. Remaining dead fields: features map and logging.directory. |
| **AD-017** | Error-Handling Gaps | Fail-open auth when `AuthConfig` extension missing (`middleware/auth.rs:15-19`); `"test-key"` API-key fallbacks in prod (`main.rs:100,109`); poisoned-lock unwraps (`package/loader.rs:48`, `plugin/wasm.rs:120,142`, `cache/semantic_cache.rs:69,93`); response-serialization unwraps (`operations/handlers.rs:52-105`); server bind panics (`main.rs:243`); `MockBackend` compiled into prod (`gates/semver.rs:187`). | Auth bypass, fake credentials, and request-path panics in production. | Propagate errors; auth fail-closed when configured; `#[cfg(test)]` mock | Resolved (2026-08-02) |
| **AD-018** | Shell Path Policy | canonicalize_within validates the argument string, but the spawned child re-resolves the path at open time: a local attacker who swaps a symlink between validation and exec escapes allowed_read_directories (TOCTOU). Flag-carried paths (-f/--file=) and commands outside FILE_READING_COMMANDS now receive path checks for known flags (2026-08-26), but arbitrary flag values and non-listed commands still execute without path policy. | Local file reads outside the sandbox root under specific conditions. Design ratified in `docs/adrs/adr-041-toctou-safe-shell-path-policy.md` (staging default + Linux openat2 hard mode). Expand per-command arg schemas as commands join the file-reading set. | Resolved (2026-08-25): staging implemented on all platforms per ADR-041 (identity-checked handle copies, argv rewrite, drop-guard + stale sweep). openat2 hard mode deferred pending Linux CI - see ADR-041 implementation status. |
| **AD-019** | Replay Re-execution | ReplayGate verifies snapshot structure, JSON validity, and declared sha256 integrity (AD-006), but snapshots are never re-executed nor semantically compared against a fresh run. Invariant 2 '100% replay fidelity' is verified structurally, not behaviorally. | A semantically divergent but well-formed payload could certify as replay-compatible. Design ratified in `docs/adrs/adr-042-replay-re-execution-harness.md` (Snapshot Payload v2 + CassetteProvider + normalized trace diff; also fixes the vacuous zero-snapshot pass). | Resolved (2026-08-25): Snapshot Payload v2 + CassetteProvider + normalized trace diff implemented per ADR-042; golden corpus committed and gate-verified; vacuous zero-snapshot pass eliminated. |

---

## Governance Rules for Debt

1. Every deliberate architectural trade-off made during an implementation phase **must** be logged with an `AD-xxx` ID.
2. Architecture Debt items cannot be closed without an empirical benchmark or PR demonstrating resolution.

---

## Detailed Forensic Trace: Compiler Optimization, Replay Invariants & Production Wiring (2026-08-01)

> Source: knowledge-graph trace of `fusion-router` (graphify-out, 5,613 nodes / 11,081 edges), verified against source. See AD-005..AD-008 above for the condensed matrix.

### 1. Executive Finding

FusionRouter contains robust, well-tested implementations of graph optimization passes (ADR-020), deterministic hashing (ADR-019), and release-gate evaluators — **but the entire optimization and determinism-verification pipeline is currently dormant in production.** Production safety relies on static strategy invariants and test-suite golden snapshots rather than runtime graph verification or active compiler pass transformations.

### 2. Subsystem Analysis

#### A. Compiler Optimization Pipeline (`src/compiler/optimization/mod.rs`)
- **Status:** Dormant (test-only).
- `FanOutConsolidationPass` (merges single-consumer/adjacent fan-outs) and `DeadNodeEliminationPass` (forward-BFS reachability pruning) are fully implemented and locked by golden tests in **`tests/golden/optimization.rs`** (`test_removes_disconnected_node` L140, `test_keeps_barrier` L160, `test_keeps_reducer` L169, `test_keeps_feedback_loop` L178, `test_fanout_consolidation_*` L347+).
- `OptimizationPipeline` is instantiated **only inside `#[cfg(test)]`** (`mod.rs:291,299,310`). It is never invoked during server pipeline execution (`src/server/pipeline.rs`) or runtime strategy resolution (`src/executor/mod.rs`).
- `BudgetOptimisationPass` (`src/compiler/passes/legacy_passes.rs:76`) is likewise defined and unit-tested (`tests/golden/compiler.rs:78,99`) but not registered in any production pipeline; its `preconditions()`/`postconditions()` trait hooks (`optimization/mod.rs:23-33`) are never called by `run()`.
- **Latent fault lines (dormant while the pipeline is asleep):**
  - `DeadNodeEliminationPass` assumes `nodes.first()` is the sole BFS root — insertion-order dependent.
  - `to_execution_graph()` silently drops `(true, _)` edges out of `FanOut`/`Barrier` nodes (`src/compiler/ir/primitive_ir.rs:227-261`) without diagnostics; orphaned successors become zero-in-degree roots.
  - The `run()` snapshot (`let snapshot = current.clone();`) is never used to restore state on error — abort-only transaction; snapshot is dead code in release builds.

#### B. Scheduler & Execution Engine (`src/scheduler/`)
- **Status:** Active.
- `WorkQueue::new()` seeds **every** node with `total_incoming == 0` into `ready` (`src/scheduler/work_queue.rs:40-44`); `DefaultScheduler` drains them and spawns concurrently via `buffer_unordered(16)` (`src/scheduler/default.rs:170-173`); `get_ready()` truncates to a hardcoded 16-node limit (`work_queue.rs:94-98`).
- **Concurrency risk:** any zero-in-degree node executes immediately in parallel. Dropped-edge conversion artifacts (see A) would race as entry points. `Barrier` semantics (`min_completion`, `timeout`, `on_failure`) have no `ExecutionNodeKind` — they die at conversion; only the DAG join structure survives.

#### C. Determinism, Replay & Telemetry (`src/release/gates/`, `src/session/`)
- **Status:** Scaffolding / partial.
- `primitive_graph_hash`: populated as `0` across the server compilation path (`src/compiler/mod.rs:101`, `src/server/handlers.rs:257`, `src/planner/resolver/capability/lowerer.rs:42,117`). `graph_hash_count` telemetry (`handlers.rs:362-366`) buckets production traffic into all-zero strings.
- `DeterminismGate`: `RealDeterminismBackend` hard-codes `GateError::ToolNotAvailable` (scaffold comment at `determinism.rs:20`), `required: false` — the only hash-comparing gate is permanently disabled in production.
- `ReplayGate` & `ReplayEngine`: validate structural JSON envelope integrity (`schema_version <= 1`, `format_version == 1`, payload non-empty) and replay `ExecutionTrace` event logs (`src/session/replay.rs:19-30`). Neither deserializes nor compares graph content hashes.

### 3. Recommendations for Production Hardening

1. **Explicit entry-node invariant:** replace `nodes.first()` with an explicit `entry_node_id` field on `PrimitiveGraph`. Do **not** adopt topo-sort-on-insertion — feedback loops are acyclic-violating by design (`test_keeps_feedback_loop`, and `WorkQueue`'s dedicated loop-back-edge handling at `work_queue.rs:34-36`).
2. **Wire the optimization pipeline:** instantiate `OptimizationPipeline` in the compilation path behind a configurable flag (`compiler.optimization_level`).
3. **Propagate real determinism hashes:** route the actual `compute_hash()` result through `CompilationStep` to replace zero-hash stubs across telemetry and attestation.
4. **Implement pass postconditions:** invoke the declared `pass.postconditions(&original, &optimized)` checks inside `OptimizationPipeline::run()` so malformed graphs cannot escape a pass.

---

## Detailed Forensic Trace: Full-Project Audit (2026-08-01)

> Method: three parallel source audits (issue markers, dormant code, error-handling contracts) + graph-wide survey + build verification. Every HIGH-severity claim spot-verified in source. See AD-009..AD-017 for the condensed matrix.

### Verification baseline

- `cargo check --all-features`: **clean, 0 warnings**. `cargo test --all-features`: **979 tests, all pass**.
- Every issue below is therefore **latent** — nothing currently fails. The test suite certifies code paths the production server never exercises.
- Graph health: 5,613 nodes / 11,081 edges / 160 components (79 isolated nodes); god nodes `GateError` (91), `CapabilityId` (58), `PrimitiveGraph` (41), `GateId` (41).

### 1. Executive Finding

FusionRouter's release certification (AD-009/010), multi-agent execution (AD-011), event/replay subsystem (AD-012), and optimization layer (AD-005) are **scaffolded but not operative**: gates evaluate fabricated evidence, strategy subgraphs execute without dataflow, the event bus drops every publish, and whole subsystems (abi/target/eri, trigger, events, session, plugin/package/runtime) are unreachable from `main.rs`. A blanket `#![cfg_attr(not(test), allow(dead_code))]` at `src/main.rs:1` hides the dead weight from the compiler. The one execution path that IS active (server → planner → scheduler → executor) is sound in structure but carries the risks in AD-008/011/014 (entry-node invariant, no dataflow, hardcoded caps).

### 2. Highest-Risk Findings (verified)

| # | Risk | Evidence |
|---|---|---|
| 1 | Required release gates pass by omission | `PolicyEvaluator::evaluate` filters out `GateExecution::ExecutionError` (evaluator.rs:94-97) — a required gate that errors never blocks |
| 2 | Event bus reports success while dropping every event | `let _ = self.sender.send(envelope)` with zero production subscribers (events/bus.rs:33); checkpoint writes also `let _` (consumers/checkpoint.rs:58-61) |
| 3 | Gate backends verify fixtures, not artifacts | Strategy backend fabricates a passing artifact (strategy.rs:109-120); ReplayGate validates its own fabricated constants (replay.rs:64-76) |
| 4 | Consensus/Debate/Fusion judge receives only the last member's output | Subgraph edges never consumed; control nodes are `{}` no-ops (executor/mod.rs:249-255); lowering failure silently degrades to a single LLM call (:284) |
| 5 | WASM guest controls host allocation | `vec![0u8; out_len]` before bounds-checked read (wasmtime_runtime.rs:154) — OOM DoS via a malicious module |
| 6 | Attestations cryptographically meaningless | `MockSigner` (DefaultHasher, "mock-sha256") in the signing path (signing.rs:30-77); `MockPackageVerifier` in the live attestations endpoint (main.rs:217) |
| 7 | Deep graphs silently truncated | Iteration cap 10 (pipeline.rs:163-165) → `success: false` with no diagnostic; `max_concurrent_nodes` config ignored above 16 (work_queue.rs:94) |
| 8 | Policy enforcement is a no-op | `IntentPlanner::plan` takes `_policies` and ignores them (intent_planner.rs:223); `FeatureRegistry` gates nothing at runtime |
| 9 | 4 telemetry counters registered but never incremented | `request_duration_seconds`, `errors_total`, `tokens_total`, `provider_latency_seconds` (metrics.rs:51-68); `graph_hash_count` uses unbounded 64-bit labels (handlers.rs:362-366) |

### 3. Secondary Findings (condensed)

- **Dormant subsystems:** `abi`/`target`/`eri` (zero workspace usages), `trigger/**`, `events/**`, `session/**` (except CLI trace), `capability_executor` (the only connector-resolution caller — connector resolution never runs in production), `WorkflowRegistry` loaded then never read, `DevexGraphVisualizer`/`TraceInspector`/`FeedbackCalibrator`/`ConnectorMetrics`/`UnifiedDiagnostics` (0 call sites), `OllamaProvider`, `StdioTransport`/`WebSocketTransport`, `DistributedScheduler`, `StrategyRegistry` (test-only), `WorkflowPlanner`/`DynamicPlanner` (main.rs:1 admits "stubs for future production wiring").
- **Placeholders:** `primitive_graph_hash: 0` at 8 sites vs 1 real computation whose result is discarded (compiler/mod.rs:101); response `usage` always `0/0` (pipeline.rs:238-242); fabricated response text `"Request processed successfully."` (pipeline.rs:211-223); `checkpoint_timestamp_ms: 1000` hardcoded → resume ordering arbitrary (checkpoint.rs:26); resume API version `0.1.0` vs workspace `0.13.0` (checkpoint.rs:48); `CURRENT_API_VERSION 0.1.0` vs ABI `0.2.0` (plugin/manager.rs:13-14); `"test-key"` API-key fallback (main.rs:100,109); hardcoded `RetryPolicy{2, 1000ms}` (compiler/mod.rs:71-74).
- **Error-context loss:** WASM errors coerced to `OutOfMemory` (wasmtime_runtime.rs:145,156); signature errors indistinguishable from invalid signatures (release/verifier.rs:59); WASM descriptor errors masked as valid degenerate strategies (plugin/wasm.rs:127-140); `total_cost = estimated_cost * 1000.0` unit mismatch (compiler/mod.rs:88).
- **Config parsed-but-ignored:** `connectors`, `providers[*].base_url`, `shutdown_timeout_secs`, `logging.directory` (config/mod.rs).
- **Concurrency:** poisoned-lock unwraps; TOCTOU `contains()`→`get().unwrap()` (executor/mod.rs:192); shell timeout without `kill_on_drop` (shell_tool.rs:65-70); budget commits before check (budget.rs:26-44).
- **Positive hygiene:** 0 TODO/FIXME, 0 `unsafe`, 0 `unimplemented!()` in prod, no `prometheus-metrics`/`semantic-cache` gate mismatch (all 4 Cargo features referenced). Note: `MockBackend` leaks into prod builds (gates/semver.rs:187) and `devex/commands/{info,inspect,logs,config_cmd}` are "not yet implemented" stubs wired into the CLI.

### 4. Recommended Fix Order

1. **AD-009** evaluator: treat `ExecutionError` as a failed gate, require evidence to Approve.
2. **AD-012** wire or delete the event bus — currently silent data loss.
3. **AD-010** replace fabricated gate backends with content-reading implementations.
4. **AD-011** execute subgraph edges so judge/reducer nodes receive member outputs.
5. **AD-013** bound WASM allocations to guest memory size before reads.

---

## Resolution Evidence (2026-08-01)

> Statuses above reflect the audit fixes landed on 2026-08-01, verified by the test suite (see per-item tests below). Items marked `Partially Resolved` have remaining work listed explicitly.

### AD-013 — RESOLVED (full)

- `SandboxConfig` gains `max_response_bytes: usize` (default 64 MiB) (`src/runtime/config.rs`); `WasmtimeSandboxInstance` carries the cap and rejects `out_len > max_response_bytes` with `RuntimeError::OutOfMemory` **before** allocating `vec![0u8; out_len]` (`src/runtime/wasmtime_runtime.rs`).
- `read_string` in the plugin loader bounds-checks the guest pointer against `memory.data(store).len()` and returns an error instead of panicking (`src/plugin/wasm.rs:184`).
- Tests: `tests/runtime_tests.rs` — `oversized_response_len_rejected_without_allocation`, `max_u32_response_len_rejected_without_allocation`, `response_within_cap_passes_through` (WAT modules); `src/plugin/wasm.rs` — `test_read_string_rejects_out_of_bounds_pointer` (negative pointer + past-end pointer, previously a host panic: `range start index ... out of range`).

### AD-010 — RESOLVED (full, 2026-08-02)

- **Strategy gate reads real content:** `FilesystemStrategyBackend::load` now parses the on-disk `StrategyManifest` JSON (`name`/`version`/`pattern`/`compiles_to_execution_graph`/`valid_policy`); a missing path, missing JSON file, or malformed JSON is a `GateError::ExecutionFailed`, never a fabricated pass (`src/release/gates/strategy.rs`). Tests: `test_filesystem_strategy_backend_load_reads_real_content`, `test_filesystem_strategy_backend_load_rejects_malformed_content` (5/5 strategy gate tests).
- **Replay gate reads real metadata:** `FilesystemReplayBackend::load_snapshot` parses the `SnapshotMetadataHeader` JSON from the first line (`<json header>\n<payload bytes>`); headerless files are errors, and metadata is no longer fabricated `0.10.0/format 1/schema 1` (`src/release/gates/replay.rs`). Tests: `test_filesystem_replay_backend_reads_real_metadata`, `test_filesystem_replay_backend_rejects_headerless_snapshot` (release 74/74).
- **Real cryptography:** `HmacSha256Signer` (HMAC-SHA256, constant-time compare, `algorithm = "hmac-sha256"`, base64 signatures) replaces the DefaultHasher `MockSigner` in the CLI (`src/release/signing.rs`, `src/bin/fusion.rs`); CLI sign/verify use `resolve_signing_key()` from `FUSION_SIGNING_KEY` and exit 1 with a diagnostic when unset. Tests: `test_hmac_signer_sign_and_verify_roundtrip`, `test_hmac_signer_rejects_tampered_payload`, `test_hmac_signer_rejects_wrong_key` (4/4).
- **Attestation verification is real:** `PackageVerifier` now returns `PackageVerification { schema_valid, signature_valid, semantic_valid }`; the new `ArchivePackageVerifier` loads the envelope from the archive and runs the real 4-phase verification, **refusing** (`FUSION_SIGNING_KEY not set`) rather than fabricating success; `MockPackageVerifier` and `MockSigner` are `#[cfg(test)]`-only; `main.rs` and `handlers.rs` wire the real verifier (+ `new_mock` is test-only). `AttestationViewer::list_packages`/`re_verify` reflect real verification results (`src/operations/attestation_viewer.rs`). Tests: `test_archive_package_verifier_refuses_without_key`, `test_archive_package_verifier_rejects_missing_attestation` (operations 17/17).

### AD-011 — RESOLVED (full, 2026-08-02)

- `DefaultExecutor::execute_node` (`src/executor/mod.rs`) now topologically orders subgraph nodes (incoming-edge map, ready-set iteration, cycle fallback to insertion order for the remainder) instead of iterating linearly.
- Member outputs accumulate in `node_outputs: HashMap<Uuid, Value>`; every upstream node's output is injected into the judge's request as `ChatMessage { role: "user", content: "Member output:\n{json}" }` — consensus/debate/fusion judges now see **all** member outputs, not just the last.
- Strategy result = exit-node output: when `subgraph.exit_node_id != node.id`, `output_value` is overridden from `node_outputs` — the node returns what the strategy's exit node produced.
- Strategy lowering failure now logs `tracing::warn!` ("strategy lowering failed, falling back to passthrough") instead of silently degrading (`resolve_strategy`).
- Tests: `test_consensus_judge_sees_member_outputs` (RED→GREEN with `CapturingAllProvider`), existing 7/7 executor tests; `cargo check --all-features --lib` clean.

### AD-009 — RESOLVED (full, 2026-08-02)

- `GateRunner::run_all`/`run_one` (`src/release/runner.rs`) normalize `ExecutionError` into a failed `GateResult` (identity preserved, summary = error text). A required gate that errors is classified `required_failed` and blocks `Approved` — errors can no longer pass by omission.
- `PolicyEvaluator::evaluate` logs a `tracing::warn!` if a raw `ExecutionError` reaches it (caller bypassed the runner contract) instead of dropping it silently (`src/release/evaluator.rs:94`).
- Empty evidence can no longer Approve: `let decision = if results.is_empty() { ReleaseDecision::Blocked } else ...` (`src/release/evaluator.rs`). An environment with zero gates or zero evidence is `Blocked`, never `Approved`.
- Tests: `release::runner::tests::test_runner_execution_error_converted_to_failed_result`, `test_runner_error_becomes_failed_result_with_identity`, `test_runner_run_one_error_becomes_failed_result`; evaluator `test_evaluator_required_execution_error_blocks`, `test_evaluator_empty_evidence_blocked`; release 74/74, evaluator 6/6.

### AD-012 — RESOLVED (full, 2026-08-02)

- `BroadcastEventBus::publish` returns `GateError::ExecutionFailed` when the broadcast send fails (zero receivers) instead of reporting success while dropping the event (`src/events/bus.rs`); `WasmtimeCapabilityHost::emit_event` already propagated this.
- `CheckpointProjection` propagates `create_dir_all` and `write` failures as `GateError::ExecutionFailed` instead of `let _ =` (`src/events/consumers/checkpoint.rs`).
- `ProjectionDispatcher` logs `tracing::error!` when a projection handler fails instead of swallowing it (`src/events/projection.rs:38`).
- **Production wiring (new):** `src/main.rs` now constructs the production `BroadcastEventBus` (capacity 1024), registers `LoggingProjection` via `ProjectionDispatcher` (live subscriber — every execution event is logged), and serves `POST /v1/executions` through the new `src/server/execution.rs` execution plane: `ExecutionPlane::execute` creates a session via `LifecycleManager` (in-memory `SessionStore`), compiles via `DefaultCompiler` (transactional passes + `lower_to_graph`), topologically schedules the graph, executes each node via `DefaultExecutor`, and streams `WorkflowStarted → WorkflowCompiled → NodeScheduled → NodeStarted → NodeFinished/NodeFailed → WorkflowCompleted/WorkflowFailed` onto the bus. Default strategies for all `StrategyKind`s are registered in `main.rs`.
- **Lagged/Closed listener robustness:** the projection listener now matches `RecvError` explicitly — `Lagged(n)` logs `tracing::warn!` and keeps consuming (no silent death, no missed tail), `Closed` logs `tracing::error!` and exits cleanly (`src/events/projection.rs`).
- Tests: `events::bus::tests::test_broadcast_event_bus_publish_without_subscribers_reports_error`, `events::consumers::checkpoint::tests::test_checkpoint_projection_write_failure_propagates`, `events::projection::tests::test_projection_listener_survives_lagged_receive` (capacity-1 bus, back-to-back publishes, listener must skip to latest and stay alive), `test_projection_listener_exits_when_bus_closed`, `test_logging_projection_accepts_all_events`; `server::execution::tests` 4/4 incl. HTTP route test (`test_execute_workflow_route_returns_200`). Live smoke test: server booted, `POST /v1/executions` with a real provider → 400 with clean error JSON, full 6-event stream observed in the log.
- **Roadmap note (not a defect):** `TriggerExecutionEngine`/`WebhookTriggerHandler`/`CronTriggerScheduler` dispatch paths remain unwired — the server does not yet parse trigger declarations or run webhook/cron listeners. The execution plane accepts trigger-style requests (`trigger_name`/`kind`/`payload` + workflow IR) directly.

### AD-017 — RESOLVED (full, 2026-08-02)

- `auth_middleware` fails closed (401 + `tracing::warn!`) when the `AuthConfig` extension is missing (`src/middleware/auth.rs`). Test: `test_auth_missing_config_fails_closed`.
- Provider API keys resolve via `resolve_provider_api_key` (`src/main.rs`) — missing `api_key_env` or unset env var is a **clean startup error** (`fail_startup`, exit 1 with diagnostic); the `"test-key"`/`"test-key-{name}"` fallbacks are gone from both the default target and the configured-provider loop. Tests: `tests::test_resolve_provider_api_key_*` (3).
- Poisoned-lock unwraps removed: `package/loader.rs:48` maps a poisoned capability-registry lock to `PackageError::Registry`; `plugin/wasm.rs:120,142` and `cache/semantic_cache.rs:69,93,114,156` recover via `unwrap_or_else(|e| e.into_inner())` — a poisoned lock can no longer panic the request path.
- Response-serialization unwraps removed: `operations/handlers.rs` serializes through `json_value()` which maps any `serde_json::to_value` failure to `500 + {"error": ...}` instead of panicking the handler task. Tests: `test_json_value_maps_serialization_failure_to_error_response`, `test_json_value_serializes_ok_value`.
- Server startup no longer panics on invalid bind address, bind failure, or serve failure — all three exit 1 with a `tracing::error!` diagnostic via `fail_startup` (`src/main.rs`).
- `MockBackend` is now `#[cfg(test)]`-only (`gates/semver.rs:187`); the integration suite defines its own `LocalMockSemVerBackend` (`tests/release_gate_tests.rs`) so the mock never ships in production builds.
- Tests: as listed above; full `cargo test --all-features` green (~1,075 tests, 0 failures, 25 test binaries); `cargo check --bins`/`--lib` clean (only pre-existing transitive `nom v1.2.4` future-incompat warning).

---

## Resolution Evidence (2026-08-26)

> Architectural-review remediation pass. Full workspace test suite green (~1,450 tests).

- **AD-006**: DeterminismGate is fixture-driven and required - src/release/gates/determinism.rs loads every file under 	ests/fixtures/determinism/ (two fixtures committed), compiles each twice via RealDeterminismBackend, and fails closed when none exist. ReplayGate verifies v1 payload JSON and declared sha256 (payload_hash) - src/release/gates/replay.rs::verify_payload.
- **AD-007**: live lowering hash verified at crates/fusion-compiler/src/lib.rs:420; residual zero-hash sites are #[cfg(test)].
- **AD-008**: PrimitiveGraph.entry_node_id added with serde default (src/compiler/ir/primitive_ir.rs); 	o_execution_graph returns Result and rejects dangling edges and invalid entries; DeadNodeEliminationPass roots at the declared entry.
- **AD-014**: BudgetEnvelope::record_and_check rolls back both counters on violation (crates/fusion-types/src/lib.rs); shell output capture bounded during read (src/tools/shell_tool.rs::read_capped); flag-carried paths validated (path_flag_value).
- **AD-015**: /v1/executions handler instruments requests/errors/latency (src/server/execution.rs).
- **Secrets**: AES-GCM SecretManager is now compiled in (src/security/mod.rs) and wired into key resolution: pi_key_encrypted on ProviderConfig decrypts via FUSION_MASTER_KEY, failing startup when undecryptable (src/providers/factory.rs::resolve_api_key). Plaintext keys remain supported by design; operators opt into encryption per provider.
- **Stale test removed**: 	ests/package_tests.rs exercised the deleted usion_router::package subsystem and broke cargo test --all-features since the 2026-08-24 de-phantom pass; deleted with the subsystem it tested.