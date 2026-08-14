# Sprint M4 — Release Policy Engine Implementation Plan

> **Goal:** Implement a declarative, data-driven Release Policy Engine (`PolicyEvaluator`, `ReleaseEnvironment`, `PolicyDefinition`, `WaiverSet`, `EvaluationContext`) that evaluates gate evidence (M1–M3) into a formal `ReleaseDecision`.

---

## Technical Architecture & Design Principles

- **Strict Decoupling:** Release gates emit evidence (`GateResult`); `PolicyEvaluator` evaluates environment policies without modifying gate traits or runner code.
- **Data-Driven Declarative Rules:** Policies loaded from `policy.yaml`; waivers loaded from `waivers.yaml` with mandatory stable IDs (`id: waiver-2026-0042`) and expiration checks.
- **Two-Phase Evaluator Pipeline:**
  1. *Evidence Classification:* Categorize results into required, advisory, waived, or ignored based on environment.
  2. *Policy Application:* Match active, unexpired waivers against failed required gates and compute `ReleaseDecision`.
- **Immutable Context:** All evaluations execute against `EvaluationContext` with an explicit timestamp (`DateTime<Utc>`).

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/release/policy.rs` | `ReleaseEnvironment`, `PolicyDefinition`, `EnvironmentPolicy`, default YAML policy loader |
| `src/release/waiver.rs` | `Waiver`, `WaiverSet`, `WaiverEvaluation`, expiration checker |
| `src/release/evaluator.rs` | `EvaluationContext`, `PolicySummary`, `ReleaseDecision`, `PolicyEvaluation`, `PolicyEvaluator` pipeline |
| `src/release/mod.rs` | Re-export `policy`, `waiver`, `evaluator` modules |
| `src/bin/fusion.rs` | Add `fusion gates evaluate` CLI subcommand and human-readable report formatter |
| `tests/release_policy_tests.rs` | Integration tests for policy evaluation, waivers, and environment rules |
| `tests/fixtures/policy.yaml` | Reference policy fixture |
| `tests/fixtures/waivers.yaml` | Reference waiver fixture |

---

## Task Breakdown & Checklists

### Task 1: Shared Policy Infrastructure & Types

**Files:**
- Create: `src/release/policy.rs`
- Create: `src/release/waiver.rs`
- Modify: `src/release/mod.rs`
- Create: `tests/fixtures/policy.yaml`
- Create: `tests/fixtures/waivers.yaml`

- [ ] **Step 1: Implement `ReleaseEnvironment` and `PolicyDefinition` in `src/release/policy.rs`**

Add `ReleaseEnvironment` enum (`Production`, `Staging`, `Development`, `Custom(String)`), `PolicyDefinition`, `EnvironmentPolicy`, and `load_policy_from_yaml()`.

- [ ] **Step 2: Implement `Waiver`, `WaiverSet`, and `WaiverEvaluation` in `src/release/waiver.rs`**

Add `Waiver` with mandatory `id: String`, `gate: GateId`, `artifact: Option<String>`, `expires: DateTime<Utc>`, `approved_by: String`, `is_active(&self, now: DateTime<Utc>) -> bool`, and `load_waivers_from_yaml()`.

- [ ] **Step 3: Re-export in `src/release/mod.rs`**

- [ ] **Step 4: Create fixture YAML files in `tests/fixtures/`**

- [ ] **Step 5: Verify unit tests**

Run: `cargo test release::policy` and `cargo test release::waiver`

---

### Task 2: Evidence Classifier & Evaluator Core

**Files:**
- Create: `src/release/evaluator.rs`
- Modify: `src/release/mod.rs`

- [ ] **Step 1: Implement `EvaluationContext`, `ReleaseDecision`, `PolicySummary`, `PolicyEvaluation`**

Define evaluation result data structures and decision matrix (`Approved`, `ApprovedWithWaivers`, `Blocked`).

- [ ] **Step 2: Implement `PolicyEvaluator` with Two-Phase Pipeline**

1. *Evidence Classification:* Match `GateResult` items against `require` and `advisory` gate lists for target environment.
2. *Waiver Application:* Match unexpired waivers against required failures by gate ID and artifact name. Compute `PolicySummary` and yield `ReleaseDecision`.

- [ ] **Step 3: Add inline unit tests**

Tests covering:
- All required gates passing -> `Approved`
- Required failure covered by active waiver -> `ApprovedWithWaivers`
- Required failure covered by expired waiver -> `Blocked`
- Unwaived required failure -> `Blocked`
- Advisory failure -> `Approved` with advisory warning in summary

- [ ] **Step 4: Verify unit tests**

Run: `cargo test release::evaluator`

---

### Task 3: Governance CLI Integration

**Files:**
- Modify: `src/bin/fusion.rs`

- [ ] **Step 1: Add `evaluate` subcommand to `GatesCmd` in `src/bin/fusion.rs`**

Arguments:
- `--env <ENV>` (default: `production`)
- `--policy <PATH>` (optional path to custom `policy.yaml`, defaults to built-in default policy)
- `--waivers <PATH>` (optional path to `waivers.yaml`)

- [ ] **Step 2: Implement human-readable report renderer for `PolicyEvaluation`**

Format `ReleaseDecision`, `PolicySummary`, passed gates, waived failures (with waiver ID and approval details), and advisory warnings.

- [ ] **Step 3: Add CLI unit tests**

Run: `cargo test --bin fusion`

---

### Task 4: Integration Tests & Workspace Verification

**Files:**
- Create: `tests/release_policy_tests.rs`

- [ ] **Step 1: Write integration tests in `tests/release_policy_tests.rs`**

Test end-to-end evaluation with runner results from `build_default_runner()`.

- [ ] **Step 2: Run full workspace quality checks**

Run:
1. `cargo test --lib release`
2. `cargo test --test release_policy_tests`
3. `cargo test --bin fusion`
4. `cargo clippy`

---

## Verification Plan

### Automated Test Suite
- `cargo test release::policy`
- `cargo test release::waiver`
- `cargo test release::evaluator`
- `cargo test --test release_policy_tests`
- `cargo test --bin fusion`
- `cargo clippy`

### CLI Command Execution
Command: `cargo run --bin fusion -- gates evaluate --env production`
Expected output: Renders policy evaluation report with environment, summary, decision, and detailed gate breakdown.
