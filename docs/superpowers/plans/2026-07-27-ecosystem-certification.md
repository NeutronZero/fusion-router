# Sprint M3 — Ecosystem Certification Implementation Plan

> **Goal:** Implement four static, artifact-based certification gates (`PLG-1`, `STR-1`, `PRV-1`, `CON-1`) built on a common `CertificationArtifact` abstraction and `CertificationContext`, extending the M1/M2 release governance framework.

---

## Technical Architecture & Design Principles

- **Single Composition Point:** Bootstrap owns gate registration via `build_default_runner()`. CLI never references gates directly by name.
- **Unified Fixture Infrastructure:** Shared `FixtureLoader` handles fixture discovery, file traversal, and manifest parsing across all gate backends.
- **Contract Certification over Implementation Testing:** Offline inspection of manifests, exported symbols, capability schemas, model catalog definitions, and protocol descriptors. Zero live sockets or network requests.
- **Execution Errors vs Conformance Failures:** Missing/unreadable files return `GateError::ExecutionFailed` / `GateError::ToolNotAvailable`. Failed contract assertions return `GateCheck { passed: false }` inside `GateExecution::Success(GateResult)`.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/release/fixture.rs` | Add `id` to `ManifestEntry`; add `Plugins`, `Strategies`, `Providers`, `Connectors` to `FixtureKind` |
| `src/release/fixture_loader.rs` | Support new `FixtureKind` variants in `discover_fixtures()` |
| `src/release/certification.rs` | `CertificationContext`, `CertificationArtifact` trait, and generic `CertificationBackend` utilities |
| `src/release/gates/plugin.rs` | `PluginGate`, `PluginArtifact`, `PluginBackend`, `FilesystemPluginBackend`, `MockPluginBackend` |
| `src/release/gates/strategy.rs` | `StrategyGate`, `StrategyArtifact`, `StrategyBackend`, `FilesystemStrategyBackend`, `MockStrategyBackend` |
| `src/release/gates/provider.rs` | `ProviderGate`, `ProviderArtifact`, `ProviderBackend`, `FilesystemProviderBackend`, `MockProviderBackend` |
| `src/release/gates/connector.rs` | `ConnectorGate`, `ConnectorArtifact`, `ConnectorBackend`, `FilesystemConnectorBackend`, `MockConnectorBackend` |
| `src/release/gates/mod.rs` | Re-export `plugin`, `strategy`, `provider`, `connector` gate modules |
| `src/release/bootstrap.rs` | Update `build_default_runner()` to register all 8 gates |
| `src/bin/fusion.rs` | Update CLI regression test for 8 gates |
| `tests/release_gate_tests.rs` | Add registration assertions for all 8 gates |
| `tests/fixtures/manifest.yaml` | Add fixture declarations for plugins, strategies, providers, connectors |

---

## Task Breakdown & Checklists

### Task 1: Shared Certification Infrastructure & Fixture Extension

**Files:**
- Modify: `src/release/fixture.rs`
- Modify: `src/release/fixture_loader.rs`
- Create: `src/release/certification.rs`
- Modify: `src/release/mod.rs`
- Modify: `tests/fixtures/manifest.yaml`

- [ ] **Step 1: Add `id` field to `ManifestEntry` & new `FixtureKind` variants**

In `src/release/fixture.rs`:
Add `#[serde(default)] pub id: Option<String>` to `ManifestEntry`.
Add `Plugins`, `Strategies`, `Providers`, `Connectors` to `FixtureKind`.

- [ ] **Step 2: Update `discover_fixtures` in `src/release/fixture_loader.rs`**

Support `Plugins`, `Strategies`, `Providers`, `Connectors` in `discover_fixtures()`.

- [ ] **Step 3: Create `src/release/certification.rs`**

```rust
use std::path::PathBuf;
use crate::release::gate::{GateCheck, GateError};

pub struct CertificationContext {
    pub fixture_root: PathBuf,
    pub sdk_version: semver::Version,
    pub workspace_root: PathBuf,
}

pub trait CertificationArtifact: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &semver::Version;
    fn schema_checks(&self, ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError>;
    fn contract_checks(&self, ctx: &CertificationContext) -> Result<Vec<GateCheck>, GateError>;
}
```

- [ ] **Step 4: Register `pub mod certification;` in `src/release/mod.rs`**

- [ ] **Step 5: Add certification entries to `tests/fixtures/manifest.yaml`**

- [ ] **Step 6: Verify compilation and tests**

Run: `cargo test fixture_loader`

---

### Task 2: Reference Implementation — Plugin Conformance Gate (PLG-1)

**Files:**
- Create: `src/release/gates/plugin.rs`
- Modify: `src/release/gates/mod.rs`

- [ ] **Step 1: Write `PluginGate`, `PluginArtifact`, `PluginBackend`, `FilesystemPluginBackend`, and `MockPluginBackend`**

In `src/release/gates/plugin.rs`:
- Implements `ReleaseGate` for `PluginGate` (`GateId::Plugin1`, category `GateCategory::Certification`).
- `PluginArtifact` implements `CertificationArtifact` (`schema_checks` & `contract_checks`).
- Verifies manifest, SDK semver compatibility, capability contracts, symbol exports, init hooks.
- Includes inline unit tests (`test_plugin_gate_passing`, `test_plugin_gate_invalid_sdk_ver`, `test_plugin_gate_execution_error`).

- [ ] **Step 2: Re-export in `src/release/gates/mod.rs`**

- [ ] **Step 3: Run unit tests**

Run: `cargo test plugin_gate`

---

### Task 3: Strategy Conformance Gate (STR-1)

**Files:**
- Create: `src/release/gates/strategy.rs`
- Modify: `src/release/gates/mod.rs`

- [ ] **Step 1: Write `StrategyGate`, `StrategyArtifact`, `StrategyBackend`, `FilesystemStrategyBackend`, and `MockStrategyBackend`**

In `src/release/gates/strategy.rs`:
- Implements `ReleaseGate` for `StrategyGate` (`GateId::Strategy1`, category `GateCategory::Certification`).
- `StrategyArtifact` implements `CertificationArtifact`.
- Verifies strategy descriptor, pattern uniqueness, compiler integration (valid `ExecutionGraph`), policy compatibility.
- Includes inline unit tests (`test_strategy_gate_passing`, `test_strategy_gate_compilation_failure`).

- [ ] **Step 2: Re-export in `src/release/gates/mod.rs`**

- [ ] **Step 3: Run unit tests**

Run: `cargo test strategy_gate`

---

### Task 4: Provider Conformance Gate (PRV-1) & Connector Conformance Gate (CON-1)

**Files:**
- Create: `src/release/gates/provider.rs`
- Create: `src/release/gates/connector.rs`
- Modify: `src/release/gates/mod.rs`

- [ ] **Step 1: Write `ProviderGate` in `src/release/gates/provider.rs`**

- Implements `ReleaseGate` for `ProviderGate` (`GateId::Provider1`, category `GateCategory::Certification`).
- `ProviderArtifact` implements `CertificationArtifact`.
- Verifies provider manifest, model catalog definition, pricing metadata schema, timeout/retry descriptors, auth schema.
- Includes inline unit tests.

- [ ] **Step 2: Write `ConnectorGate` in `src/release/gates/connector.rs`**

- Implements `ReleaseGate` for `ConnectorGate` (`GateId::Connector1`, category `GateCategory::Certification`).
- `ConnectorArtifact` implements `CertificationArtifact`.
- Verifies protocol schema, serialization compatibility, health endpoint declaration, credential descriptor.
- Includes inline unit tests.

- [ ] **Step 3: Re-export both modules in `src/release/gates/mod.rs`**

- [ ] **Step 4: Run unit tests**

Run: `cargo test provider_gate` and `cargo test connector_gate`

---

### Task 5: Bootstrap Wiring & Integration Tests

**Files:**
- Modify: `src/release/bootstrap.rs`
- Modify: `tests/release_gate_tests.rs`
- Modify: `src/bin/fusion.rs`

- [ ] **Step 1: Update `build_default_runner()` in `src/release/bootstrap.rs`**

Register all 8 gates in documented order:
1. `SDK-1` (Compatibility)
2. `RPL-1` (Replay)
3. `UPG-1` (Upgrade)
4. `DET-1` (Determinism)
5. `PLG-1` (Certification)
6. `STR-1` (Certification)
7. `PRV-1` (Certification)
8. `CON-1` (Certification)

- [ ] **Step 2: Update integration tests in `tests/release_gate_tests.rs`**

Assert `runner.gates().len() == 8` and verify all gate IDs match.

- [ ] **Step 3: Update CLI regression tests in `src/bin/fusion.rs`**

Assert `fusion gates list` output contains all 8 gate IDs (`SDK-1`, `RPL-1`, `UPG-1`, `DET-1`, `PLG-1`, `STR-1`, `PRV-1`, `CON-1`).

- [ ] **Step 4: Full Workspace Verification**

Run:
1. `cargo test --lib release`
2. `cargo test --test release_gate_tests`
3. `cargo test --bin fusion`
4. `cargo run --bin fusion -- gates list`

---

## Verification Plan

### Automated Test Suite
- `cargo test release::certification`
- `cargo test release::gates::plugin`
- `cargo test release::gates::strategy`
- `cargo test release::gates::provider`
- `cargo test release::gates::connector`
- `cargo test --test release_gate_tests`
- `cargo test --bin fusion`

### CLI Output Verification
Command: `cargo run --bin fusion -- gates list`
Expected output: Lists 8 gates formatted by category.
