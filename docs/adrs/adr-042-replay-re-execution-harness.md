# ADR-042: Replay Re-Execution Harness

- **Status:** Accepted (implemented 2026-08-25)
- **Date:** 2026-08-25
- **Applies to:** replay gate (`src/release/gates/replay.rs`), replay engine (`src/session/replay.rs`), execution event model (`src/events/payload.rs`), fixtures (`tests/fixtures/snapshots/`, `tests/fixtures/manifest.yaml`)
- **Charter:** AF-003 Invariants 2 & 3 (replay compatibility, deterministic compilation), Platform Invariant #2 "100% replay fidelity" (`docs/architecture/status.md`), Debt Register AD-019

## Context

Platform Invariant #2 declares *100% replay fidelity* a release-blocking gate.
Today the gate certifies none of it:

1. `ReplayGate` verifies structure (header fields, JSON validity, declared
   sha256 — the latter added 2026-08-26) but never executes a snapshot.
2. `ReplayEngine::replay_inspection` returns the last `ExecutionFinished`
   event without re-executing or comparing anything.
3. The repository contains **zero** `.snap` files, so the gate passes
   vacuously with `"No snapshots to check"`.

A well-formed, hash-consistent payload whose semantics diverge from what the
current pipeline would produce therefore ships certified. The gap is twofold:
there is no golden corpus, and there is no behavioral comparator.

Naive re-execution cannot work: providers are nondeterministic by nature, so
a recorded trace can only be reproduced if provider responses themselves are
part of the record.

## Decision

### 1. Snapshot Payload v2

`schema_version: 2` snapshots carry everything needed for behavioral
verification:

```json
{
  "workflow_ir": "<WorkflowIR canonical JSON>",
  "policy_version": 7,
  "provider_cassette": [
    { "model": "openrouter/llama-3", "response": { /* ChatCompletionResponse */ } }
  ],
  "expected_events": [ /* events/payload.rs ExecutionEvent, snake_case */ ]
}
```

The header gains nothing; the existing `payload_hash` (sha256 of payload
bytes) already covers integrity.

### 2. Volatile-field normalization

Comparison runs on normalized events: `duration_ms`, `total_duration_ms`,
`cost`, `*_tokens`, and `prompt_bytes` are stripped on both sides before the
diff. Identity fields — event type sequence, `node_id`, `node_kind`,
`provider`, `model`, dependency lists, retry attempts — must match exactly.
Rationale: wall-clock timing and pricing drift are not semantic changes;
everything else is.

### 3. Gate algorithm (`schema_version == 2`)

```
parse payload -> verify payload_hash
compile workflow_ir through build_compiler(policy_version)
assert primitive_graph_hash + node_count + edge_count match WorkflowCompiled in expected_events
execute graph against CassetteProvider(cassette)          // no network
collect emitted events -> normalize both sides
require exact sequence equality
on mismatch: report first divergence (index, expected, actual)
```

Cassette matching is strict-order: execution under the WorkQueue's topological
order is deterministic (Invariants 3–4), so call k must consume cassette entry
k. A length mismatch is a failure — it means provider-contract drift, exactly
the class of regression this gate exists to catch.

### 4. Snapshot producer

`fusion release snapshot-record --workspace . --request-file req.json`

Boots the real pipeline (chat path, `build_compiler`) against a
`CassetteProvider`, records normalized events, computes `payload_hash`, writes
`<json header>\n<payload bytes>`. Snapshots land in
`tests/fixtures/snapshots/<id>.snap`; each release line gets an entry in
`tests/fixtures/manifest.yaml` under `snapshots:` (the manifest section exists
today but points at directories that were never created).

Initial corpus: one snapshot per built-in strategy (`single`, `consensus`,
`debate`, plus one tool-invoking workflow) at the current version.

### 5. Vacuous pass eliminated

- Manifest declares N snapshot entries → fewer than N loadable snapshots is a
  **gate error**, not success.
- Zero configured snapshots: advisory warning until v1.0, then the gate
  requires a non-empty corpus (`required: true` already set).

### 6. Governance

Snapshots are versioned contracts. An intentional ABI/compiler change that
breaks snapshots requires: (a) re-recording via the producer CLI, (b) a note
in the ADR/change PR citing which invariant changed, and (c) either a waiver
or clean pass from the release policy engine — same flow as any other gate
failure. Silent snapshot regeneration in CI is prohibited.

## Alternatives Considered

- **Deep-hash compare without execution** (hash the stored outputs): catches
  storage corruption but not compiler/executor regressions. Already covered by
  `payload_hash`; rejected as insufficient alone.
- **LLM-as-judge similarity over traces:** a nondeterministic verifier cannot
  certify a determinism invariant. Rejected.
- **Property-based IR fuzzing** to generate workflows on the fly instead of a
  fixed corpus: strong complement for coverage, but it verifies compilation,
  not replay compatibility specifically. Future work alongside this ADR.

## Consequences

- Replay fidelity becomes behaviorally verified end-to-end: compile equality
  (Invariant 3) *and* execution-trace equality (Invarant 2), per release.
- Snapshots become maintained artifacts; strategy/ABI changes require explicit
  re-recording, surfacing hidden coupling between releases.
- CI cost: cassette replay only — no network, seconds per snapshot.
- Code touched: `verify_payload` gains the v2 branch; new `CassetteProvider`
  (test-infra crate location); normalization fn beside `events/payload.rs`;
  producer subcommand in `src/bin/fusion.rs`.
- `ExecutionEvent` (payload enum) already derives `Serialize/Deserialize` —
  no schema migration needed, only the normalization helper.
- Tests required for closure:
  1. Record → gate-pass round trip for each initial-corpus snapshot.
  2. One-byte response mutation → `payload_hash` failure.
  3. Cassette reorder/truncation → trace mismatch reported with divergence index.
  4. Manifest entry with missing file → vacuous-pass error.
  5. Compiler change flipping node order → `WorkflowCompiled` hash mismatch
     caught before execution.

## Debt Register Link

Closes AD-019 upon merge with tests 1–5 green. AD-006's remaining clause
("replay payloads never re-executed") is superseded by this ADR.

## Implementation Status (2026-08-25)

- src/release/snapshot.rs: SnapshotPayloadV2, CassetteProvider (strict-order,
  model-drift detection), volatile-field normalization, trace diff with
  first-divergence reporting, ReplayHarness, record_trace, verify_payload_v2.
- Gate integration: schema_version >= 2 payloads are behaviorally verified
  (recompile -> cassette replay -> normalized trace diff, 120s timeout); the
  compatibility check now admits schema <= 2; declared-but-missing snapshots
  fail closed (vacuous-pass eliminated).
- Corpus: three committed v2 snapshots (single, chain_two_step,
  consensus_three_member) under tests/fixtures/snapshots/v0.14/, produced by
  cargo run --example record_replay_snapshots through the real execution
  plane. Manifest points at v0.14.
- The producer CLI subcommand from the Decision section landed as an example
  binary instead of a usion subcommand; promotion to the CLI is mechanical
  once the bin's arg parsing is next touched.
- Tests: real-corpus gate run asserts behavioral verification happened;
  cassette order-drift rejection; vacuous-manifest fail-closed; normalization/
  diff unit tests; full workspace suite green (1,714 tests).