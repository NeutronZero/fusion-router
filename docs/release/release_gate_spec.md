# Release Gate Specification

> **Purpose:** Define testable release criteria for each Platform Invariant.
> **Status:** Ratified — v0.11 planning phase
> **Applies to:** All releases v0.11 onward

---

## 1. Overview

Each Platform Invariant (defined in the [v0.11 Roadmap](../roadmap-v0.11.md#2-platform-invariants--v10-release-gates)) maps to one or more automated release gates. A gate **must pass** before a release candidate can proceed to the next stage. Any gate failure is release-blocking unless explicitly waived via ADR.

```
                    ┌──────────────────┐
                    │  Unit + Goldens   │
                    └────────┬─────────┘
                             │
                    ┌────────▼─────────┐
                    │  Platform Gates   │  ←── this document
                    └────────┬─────────┘
                             │
                    ┌────────▼─────────┐
                    │  Release Decision │
                    └──────────────────┘
```

Gates are distinct from routine CI — they test cross-version, cross-component, and behavioral properties that unit tests alone cannot verify.

---

## 2. Gate Definitions

### Gate SDK-1 — Public API Compatibility

| Field | Value |
|-------|-------|
| Invariant | Public SDK compatibility |
| Validation | `cargo test --test sdk_compatibility` — runs against the prior minor version's public API surface using a published reference |
| Blocking condition | Any public API signature change without an explicit `#[api_version("...")]` annotation or migration shim |
| Integration point | Pre-release CI pipeline; runs against the prior minor version's API snapshot |
| Existing tests | `tests/unit/phase_invariants.rs` (capability registry immutability, contract checks) |
| New tooling required | API snapshot tool (`cargo fusion api-snapshot`), semver diff checker |
| Failure action | Block release; require ADR and migration path for intentional breaks |

---

### Gate RPL-1 — Replay Compatibility

| Field | Value |
|-------|-------|
| Invariant | Replay compatibility |
| Validation | `cargo test --test replay_compatibility` — loads snapshots recorded by version N, replays them on version N+1, asserts identical output (modulo provider non-determinism) |
| Blocking condition | Any replay failure or output divergence exceeding the provenance-attributed delta |
| Integration point | Pre-release CI pipeline; runs against a snapshot corpus from the prior minor version |
| Existing tests | `tests/replay/` — replay module with snapshot-based tests |
| New tooling required | Snapshot corpus management, version-tagged snapshot fixtures |
| Failure action | Block release; requires fix or ADR documenting intentional replay format change |

---

### Gate SES-1 — Session Migration Safety

| Field | Value |
|-------|-------|
| Invariant | Session migration safety |
| Validation | `cargo test --test session_migration` — serializes session state with version N format, deserializes with version N+1 format, asserts all fields survive round-trip |
| Blocking condition | Any deserialization failure, data loss, or silent field truncation |
| Integration point | Pre-release CI pipeline |
| Existing tests | `tests/unit/session_phase_invariants.rs` — session state invariants |
| New tooling required | Version-tagged session serialization fixtures |
| Failure action | Block release; may require migration layer or format version bump |

---

### Gate DET-1 — Deterministic Compilation

| Field | Value |
|-------|-------|
| Invariant | Deterministic compilation |
| Validation | `cargo test --test deterministic_compilation` — compiles identical `WorkflowIR` input twice, asserts `ExecutionGraph` hash identity |
| Blocking condition | Any hash mismatch between identical inputs |
| Integration point | Standard CI (gate runs on every PR affecting compiler code) |
| Existing tests | `tests/strategy_sdk/lowering/deterministic.rs` — per-strategy determinism checks |
| New tooling required | Full-pipeline determinism harness (Planner → Compiler → ExecutionGraph) |
| Failure action | Block PR; regression must be fixed or proven intentional via ADR |

---

### Gate SEM-1 — Stable Execution Semantics

| Field | Value |
|-------|-------|
| Invariant | Stable execution semantics |
| Validation | `cargo test --test golden_execution` — golden test suite with pinned provider responses; asserts execution output matches golden file |
| Blocking condition | Any output drift from golden files (provider non-determinism excluded via pinned responses) |
| Integration point | Pre-release CI pipeline; full golden suite run |
| Existing tests | `tests/golden/` — compiler, DAG, strategy, plugin, optimization, and tool execution golden tests |
| New tooling required | Provider response recording/capture-replay for deterministic golden execution; golden diff tool |
| Failure action | Block release; requires golden file update with documented justification |

---

### Gate CON-1 — Connector Conformance

| Field | Value |
|-------|-------|
| Invariant | Connector conformance |
| Validation | `cargo fusion certify-connector` run against every certified connector on the target runtime version |
| Blocking condition | Any certified connector fails the conformance suite |
| Integration point | Pre-release CI pipeline; runs after build, before release artifact publication |
| Existing tests | Plugin compliance tests (`tests/strategy_sdk/plugin/compliance.rs`), connector `CapabilityPlugin` tests |
| New tooling required | `cargo fusion certify-connector` subcommand; connector conformance test harness |
| Failure action | Block release; decertify connector or fix before shipping |

---

### Gate POL-1 — Policy Determinism

| Field | Value |
|-------|-------|
| Invariant | Policy determinism |
| Validation | `cargo test --test policy_determinism` — evaluates identical policy sets against identical requests twice, asserts identical decisions |
| Blocking condition | Any decision mismatch between identical evaluations |
| Integration point | Standard CI (gate runs on every PR affecting policy code) |
| Existing tests | `tests/unit/policy_phase_invariants.rs` — policy compilation and evaluation invariants |
| New tooling required | Policy golden test suite with pinned decision outputs |
| Failure action | Block PR; regression must be fixed before merge |

---

### Gate UPG-1 — Upgrade/Rollback Safety

| Field | Value |
|-------|-------|
| Invariant | Upgrade/rollback safety |
| Validation | `cargo test --test upgrade_safety` — installs version N, performs operations, upgrades to N+1, verifies state, downgrades to N, verifies recovery |
| Blocking condition | Any data loss, session corruption, configuration loss, or failure to roll back |
| Integration point | Pre-release CI pipeline; full upgrade/downgrade cycle |
| Existing tests | None specific to upgrade paths |
| New tooling required | Upgrade fixture infrastructure; multi-version state generators; install/rollback harness |
| Failure action | Block release; requires migration fix or documented rollback window |

---

## 3. Gate Execution Order

Gates run in dependency order within the release pipeline:

```
┌─────────────────────────────────────────────────────┐
│                    PR Pipeline                       │
├─────────────────────────────────────────────────────┤
│  DET-1   Deterministic compilation                  │
│  POL-1   Policy determinism                         │
└─────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────┐
│              Pre-Release Pipeline                    │
├─────────────────────────────────────────────────────┤
│  SEM-1   Stable execution semantics (goldens)       │
│  CON-1   Connector conformance                      │
│  SES-1   Session migration safety                   │
│  RPL-1   Replay compatibility                       │
│  SDK-1   Public API compatibility                   │
│  UPG-1   Upgrade/rollback safety                    │
└─────────────────────────────────────────────────────┘
```

- **PR Pipeline gates** run on every pull request. Failure blocks merge.
- **Pre-Release Pipeline gates** run before release artifact publication. Failure blocks release.

---

## 4. Gate Waiver Process

A release-blocking gate failure may be waived only under the following conditions:

1. **ADR required** — The waiver must be documented as a formal Architectural Decision Record explaining:
   - Which invariant is being intentionally modified
   - Why extension or preservation is not feasible
   - The migration path for affected consumers
   - The expected duration of the waiver (single release or permanent)
2. **Version-bump required** — Intentional invariant changes that cannot be backward-compatible require a minor or major version bump per semver
3. **Consensus required** — Waiver must be approved by at least two maintainers with architectural context

Gates may **not** be waived for:
- Accidental regressions (must be fixed)
- Incomplete migration paths (must be implemented before release)
- Schedule pressure (no "ship now, fix later" for invariants)

---

## 5. Tooling Roadmap

| Gate | Tooling | Epic |
|------|---------|------|
| SDK-1 | API snapshot tool, semver diff checker | M |
| RPL-1 | Snapshot corpus management, version-tagged fixtures | M |
| SES-1 | Version-tagged session serialization fixtures | M |
| DET-1 | Full-pipeline determinism harness | M |
| SEM-1 | Provider response recording/capture-replay; golden diff tool | M |
| CON-1 | `cargo fusion certify-connector` subcommand | SDK Validation Suite |
| POL-1 | Policy golden test suite with pinned decision outputs | M |
| UPG-1 | Upgrade fixture infrastructure; install/rollback harness | M |

All gate tooling is owned by Epic M (Compatibility & Release Engineering) unless otherwise noted.

---

## 6. References

- [v0.11 Roadmap — Section 2: Platform Invariants](../roadmap-v0.11.md#2-platform-invariants--v10-release-gates)
- [ADR-027 — Architecture Conformance Testing](../docs/adr/ADR-027-architecture-conformance-testing.md) (phase invariants)
- [Architecture Debt Register](../docs/architecture/architecture_debt_register.md)
