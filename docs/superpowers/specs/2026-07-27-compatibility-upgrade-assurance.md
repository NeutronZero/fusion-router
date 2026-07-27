# Sprint M2 — Compatibility & Upgrade Assurance

> **Theme:** Ensure that upgrading FusionRouter preserves correctness, replayability, and operational safety through artifact-based release gates.
> **Status:** Design
> **Dependencies:** Sprint M1 (release gate infrastructure), `tests/fixtures/` (new)

---

## 1. Architecture

M2 introduces three new release gates that reuse M1's framework without adding new infrastructure. Every gate follows the same pattern: `impl ReleaseGate`, backend trait, mock backend, focused test suite.

```
src/release/gates/
├── mod.rs              ← re-exports (semver + replay + upgrade + determinism)
├── semver.rs           ← existing (M1)
├── replay.rs           ← ReplayGate (NEW)
├── upgrade.rs          ← UpgradeGate (NEW)
└── determinism.rs      ← DeterminismGate (NEW)

tests/
├── common/mod.rs       ← shared fixture discovery helpers (NEW)
├── fixtures/
│   ├── manifest.yaml   ← fixture metadata (NEW)
│   ├── configs/        ← UpgradeGate fixtures
│   │   ├── v0.9/
│   │   └── v0.10/
│   └── snapshots/      ← ReplayGate fixtures
│       └── v0.10/
└── release_gate_tests.rs  ← existing + M2 integration tests
```

### Invariant

All governance logic lives in `src/`. CLI is a thin renderer. The `bootstrap()` function in `src/release/bootstrap.rs` owns gate registration — the CLI never needs to know which gates exist.

---

## 2. Gate Category

Add `Replay` variant to the existing `GateCategory` enum:

```rust
pub enum GateCategory {
    Compatibility,
    Replay,
    Upgrade,
    Determinism,
    Certification,
}
```

`gates list` output is ordered by category (the enum variant order above), then by `GateId` within each category.

---

## 3. ReplayGate (RPL-1)

### Identity

| Field | Value |
|-------|-------|
| `GateId` | `Replay1` |
| `GateCategory` | `Replay` |
| Required | Yes |
| Name | "Replay Compatibility" |
| Description | "Verify replay snapshots remain readable and structurally valid" |

### Pipeline

```
discover snapshots (via manifest or directory scan)
    ↓
load metadata
    ↓
deserialize with current code
    ↓
validate invariants → individual GateChecks per category
    ↓
aggregate into GateResult
```

### Snapshot Invariant Categories

Each produces a named `GateCheck`:

| Check Name | What It Verifies |
|---|---|
| `metadata-version` | Metadata section exists and contains version/schema fields |
| `format-version` | Snapshot format version is within supported range |
| `schema-version` | Schema version is compatible with current code |
| `payload-deserialization` | Snapshot payload deserializes without errors |
| `required-sections` | All required structural sections are present |
| `unknown-critical-fields` | No unknown critical fields are rejected |

### Backend

```rust
pub struct SnapshotData {
    pub metadata: SnapshotMetadata,
    pub payload: Vec<u8>,
}

pub struct SnapshotMetadata {
    pub version: semver::Version,
    pub format_version: u32,
    pub schema_version: u32,
    pub producer_version: String,
}

pub struct ReplayContext {
    pub root: std::path::PathBuf,
    pub manifest: Option<FixtureManifest>,
    pub version: Option<semver::Version>,
}

pub trait ReplayBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn discover_snapshots(&self, ctx: &ReplayContext) -> Result<Vec<SnapshotData>, GateError>;
    fn load_snapshot(&self, path: &std::path::Path) -> Result<SnapshotData, GateError>;
}

pub struct FilesystemReplayBackend;

// MockBackend under #[cfg(test)]
impl ReplayBackend for MockReplayBackend { ... }
```

---

## 4. UpgradeGate (UPG-1)

### Identity

| Field | Value |
|-------|-------|
| `GateId` | `Upgrade1` |
| `GateCategory` | `Upgrade` |
| Required | No (advisory) |
| Name | "Configuration Upgrade" |
| Description | "Verify historical configs load correctly through the current parser" |

### Pipeline

```
discover configs (via manifest)
    ↓
load each config
    ↓
parse with current AppConfig deserializer
    ↓
run validate()
    ↓
compare actual outcome vs expected outcome
    ↓
aggregate into GateResult
```

### Fixture Expected Outcomes

Each config fixture specifies its expected outcome in the manifest:

```yaml
expected:
  outcome: pass   # pass | warning | fail
```

The gate compares **actual vs expected**:
- A `pass` fixture that produces errors → failure
- A `fail` fixture that produces no errors → failure
- A `warning` fixture that produces errors → pass (but listed separately)

### Backend

```rust
pub struct ConfigFixture {
    pub version: semver::Version,
    pub path: std::path::PathBuf,
    pub expected: ExpectedOutcome,
}

pub enum ExpectedOutcome {
    Pass,
    Warning,
    Fail,
}

pub struct UpgradeContext {
    pub root: std::path::PathBuf,
    pub manifest: Option<FixtureManifest>,
}

pub trait UpgradeBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn discover_configs(&self, ctx: &UpgradeContext) -> Result<Vec<ConfigFixture>, GateError>;
    fn load_config(&self, fixture: &ConfigFixture) -> Result<String, GateError>;
}

pub struct FilesystemUpgradeBackend;

// MockBackend under #[cfg(test)]
impl UpgradeBackend for MockUpgradeBackend { ... }
```

---

## 5. DeterminismGate (DET-1)

### Identity

| Field | Value |
|-------|-------|
| `GateId` | `Determinism1` |
| `GateCategory` | `Determinism` |
| Required | No (advisory) |
| Name | "Planner Determinism" |
| Description | "Verify same planner input produces identical execution graphs" |

### Pipeline

```
load request fixture
    ↓
fresh planner instance (no shared state)
    ↓
compile → WorkflowIR → ExecutionGraph → hash
--------
fresh planner instance (no shared state)
    ↓
compile → WorkflowIR → ExecutionGraph → hash
--------
compare hashes → single GateCheck
```

### Key Constraints

- Each compilation uses a **fresh, isolated planner/compiler context** — no cached results, no shared mutable state.
- Comparison uses the canonical `ExecutionGraph` hash (via `compute_hash()` or stable serde serialization), not object identity.
- The hash must be deterministic across compiler versions — if the hash function itself changes, that's a documented breaking change.

### Backend

```rust
pub struct DeterminismContext {
    pub root: std::path::PathBuf,
    pub request_fixture: String,
}

pub trait DeterminismBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn compile_fixture(&self, ctx: &DeterminismContext) -> Result<u64, GateError>;
}

pub struct RealDeterminismBackend;

// MockBackend under #[cfg(test)]
impl DeterminismBackend for MockDeterminismBackend { ... }
```

---

## 6. Bootstrap & Registration

`src/release/bootstrap.rs` gains `build_default_runner()` which registers all four gates:

```rust
pub fn build_default_runner() -> GateRunner {
    let mut runner = GateRunner::new(vec![
        Box::new(SemVerGate::new(baseline, crate_path)),
        Box::new(ReplayGate::new(replay_backend, replay_config)),
        Box::new(UpgradeGate::new(upgrade_backend, upgrade_config)),
        Box::new(DeterminismGate::new(determinism_backend, determinism_config)),
    ]);
    runner
}
```

The CLI calls `build_default_runner()` — it never references individual gates by name.

---

## 7. Fixture Manifest

```yaml
# tests/fixtures/manifest.yaml

configs:
  - version: "0.9"
    path: configs/v0.9
    expected:
      outcome: pass

  - version: "0.10"
    path: configs/v0.10
    expected:
      outcome: pass

snapshots:
  - version: "0.10"
    path: snapshots/v0.10
```

---

## 8. Shared Test Utilities (`tests/common/`)

```rust
/// Load and parse fixture manifest from path.
pub fn load_fixture_manifest(path: &str) -> Result<FixtureManifest>;

/// Discover fixture entries from manifest, in deterministic order
/// (manifest order → version → path).
pub fn discover_fixtures(
    manifest: &FixtureManifest,
    kind: FixtureKind,
) -> Vec<FixtureEntry>;
```

`FixtureManifest`, `FixtureEntry`, `FixtureKind` types live in `tests/common/mod.rs`.

---

## 9. Testing Strategy

| Layer | Scope | Tests |
|---|---|---|
| Backend unit | Each gate's mock backend returns expected results | 3–5 per gate |
| Fixture helpers | `load_fixture_manifest()`, `discover_fixtures()` | 3–4 |
| Runner registration | Bootstrap registers all 4 gates | 1 |
| Integration | Mock backends through runner → report | Extend existing test file |
| CLI regression | `fusion gates list` output contains SDK-1, RPL-1, UPG-1, DET-1 | 1 |

---

## 10. File Changes Summary

| File | Action |
|---|---|
| `src/release/gates/mod.rs` | Modify — add `replay`, `upgrade`, `determinism` modules |
| `src/release/gates/replay.rs` | Create — `ReplayGate`, `ReplayBackend`, `FilesystemReplayBackend`, `MockReplayBackend` |
| `src/release/gates/upgrade.rs` | Create — `UpgradeGate`, `UpgradeBackend`, `FilesystemUpgradeBackend`, `MockUpgradeBackend` |
| `src/release/gates/determinism.rs` | Create — `DeterminismGate`, `DeterminismBackend`, `RealDeterminismBackend`, `MockDeterminismBackend` |
| `src/release/gate.rs` | Modify — add `GateCategory::Replay` variant |
| `src/release/bootstrap.rs` | Modify — add `build_default_runner()`, register all 4 gates |
| `tests/common/mod.rs` | Create — `FixtureManifest`, `FixtureEntry`, `FixtureKind`, `load_fixture_manifest()`, `discover_fixtures()` |
| `tests/fixtures/manifest.yaml` | Create — fixture metadata |
| `tests/fixtures/configs/v0.9/` | Create — sample config fixtures |
| `tests/fixtures/configs/v0.10/` | Create — sample config fixtures |
| `tests/fixtures/snapshots/v0.10/` | Create — sample snapshot fixtures |
| `tests/release_gate_tests.rs` | Modify — add M2 integration tests |

---

## 11. Deferred (Post-M2 Integration)

- CI integration (`fusion gates check` in GitHub Actions) — small operational task, not a sprint
- Behavioral replay / output comparison
- Plugin / connector / strategy certification
- Waiver management
- Policy-gated releases

---

## 12. Roadmap

| Sprint | Theme |
|---|---|
| **M1 ✓** | Release gate infrastructure (framework, runner, feature gates, SemVer) |
| **M2** | Compatibility assurance (Replay, Upgrade, Determinism) |
| **M3** | Ecosystem certification (plugins, connectors, strategies) |
| Later | Behavioral replay, waiver system, policy-gated releases, CI integration |
