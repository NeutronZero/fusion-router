# Sprint M1 — Release Gate Foundation

> **Theme:** Making release gates executable through feature-flag infrastructure, SemVer enforcement, and a pluggable gate runner.
> **Status:** Design
> **Dependencies:** Stage 1 foundation (Sprints 1.1–1.5), `cargo semver-checks` (external tool)

---

## 1. Architecture

```
fusion-core library
    │
    ├── feature_gate/     ← runtime feature state + lifecycle
    ├── release/          ← governance + release gates
    │   └── gates/        ← individual gate implementations
    ├── config/           ← persistence + configuration model
    └── lib.rs            ← pub mod exports
            │
            ├─── fusion-server (HTTP, runtime, background tasks)
            │
            └─── fusion-cli (cargo fusion subcommands)
                     │
                     ├── gates check/list/explain
                     ├── features list
                     └── (future: certify, replay-check, doctor)
```

**Invariant:** All governance logic lives in `fusion-core`. CLI is a thin renderer. Server never imports CLI code.

---

## 2. Feature Flag Infrastructure

### 2.1 Core Types

```rust
// src/feature_gate/mod.rs

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeatureFlag {
    Streaming,
    Replay,
    ConnectorHealth,
    SemanticCache,
    WasmPlugins,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureDefinition {
    pub id: FeatureFlag,
    pub introduced: semver::Version,
    pub removed: Option<semver::Version>,
    pub stability: Stability,
    pub default_enabled: bool,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stability {
    Experimental,
    Stable,
    Deprecated,
}
```

### 2.2 Feature Registry

```rust
pub struct FeatureRegistry {
    registry: HashMap<FeatureFlag, FeatureState>,
    definitions: &'static [FeatureDefinition],
}

struct FeatureState {
    enabled: bool,
    overridden: bool, // true if explicitly set by config, not just default
}
```

**Key behaviors:**

- `new()` initializes from `FeatureDefinition` defaults
- `apply_config()` merges user config overrides (hot-reloadable)
- `is_enabled(feature)` checks runtime state
- `list()` returns all features with current state + metadata
- `subscribe()` returns a `ConfigSubscriber` impl for live reload

### 2.3 Configuration Integration

In `config/default.yaml`:

```yaml
features:
  streaming:
    enabled: true
  replay:
    enabled: true
  connector-health:
    enabled: true
  semantic-cache:
    enabled: true
  wasm-plugins:
    enabled: false   # requires WASM toolchain

# Reserved for future release gate configuration (not implemented in M1)
# release:
#   gates:
#     sdk-1:
#       enabled: true
#     replay-1:
#       enabled: false
```

`AppConfig` gets:

```rust
pub struct AppConfig {
    // ... existing fields ...
    pub features: HashMap<String, FeatureConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureConfig {
    pub enabled: bool,
}
```

### 2.4 Hot-Reload Cycle

`FeatureRegistry` implements `ConfigSubscriber`:

1. `prepare(config)` → parses feature overrides, validates feature names exist
2. `commit()` → applies parsed state atomically
3. `rollback()` → discards pending changes

The `ConfigManager`'s existing two-phase commit handles feature flag updates through the same subscriber mechanism.

### 2.5 Compile-Time Bridge

Cargo features (`semantic-cache`, `wasm-plugins`) remain compile-time gates. The runtime `FeatureRegistry` can additionally disable a feature at startup. If a feature is compile-time disabled (`#[cfg]`), the runtime flag has no effect. If compile-time enabled, the runtime flag can still disable it.

```rust
impl FeatureRegistry {
    pub fn is_effectively_enabled(&self, flag: FeatureFlag) -> bool {
        if !self.compile_time_enabled(flag) {
            return false;
        }
        self.state(flag).enabled
    }
}
```

---

## 3. Release Gate Framework

### 3.1 Core Trait

```rust
// src/release/gate.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GateId {
    Sdk1,
    Replay1,
    Upgrade1,
    Determinism1,
}

impl GateId {
    pub fn as_str(&self) -> &'static str {
        match self {
            GateId::Sdk1 => "SDK-1",
            GateId::Replay1 => "RPL-1",
            GateId::Upgrade1 => "UPG-1",
            GateId::Determinism1 => "DET-1",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "SDK-1" => Some(GateId::Sdk1),
            "RPL-1" => Some(GateId::Replay1),
            "UPG-1" => Some(GateId::Upgrade1),
            "DET-1" => Some(GateId::Determinism1),
            _ => None,
        }
    }
}

impl fmt::Display for GateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for GateId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GateId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        GateId::from_str(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown gate: {s}")))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateContext {
    pub workspace_root: PathBuf,
    pub baseline_version: Option<semver::Version>,
    pub features: Arc<FeatureRegistry>,
    pub config: Arc<AppConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    pub gate_id: GateId,
    pub passed: bool,
    pub summary: String,
    pub details: Vec<GateCheck>,
    pub duration: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateCheck {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateMetadata {
    pub id: GateId,
    pub category: GateCategory,
    pub required: bool,
    pub introduced: semver::Version,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateCategory {
    Compatibility,
    Determinism,
    Upgrade,
    Certification,
}

pub trait ReleaseGate: Send + Sync {
    fn id(&self) -> GateId;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn metadata(&self) -> GateMetadata;
    fn run(&self, ctx: &GateContext) -> Result<GateResult, GateError>;
}
```

### 3.2 Gate Runner

```rust
// src/release/runner.rs

pub struct GateRunner {
    gates: Vec<Box<dyn ReleaseGate>>,
}

impl GateRunner {
    pub fn new(gates: Vec<Box<dyn ReleaseGate>>) -> Self;
    pub fn register(&mut self, gate: Box<dyn ReleaseGate>);
    pub fn run_all(&self, ctx: &GateContext) -> Vec<GateResult>;
    pub fn run_one(&self, id: GateId, ctx: &GateContext) -> Option<GateResult>;
}
```

Discovery is explicit (not reflection-based). Gates are registered at construction time. This keeps the architecture simple and the trait bound straightforward.

**Execution order:** Gates run in registration order (FIFO). Callers control ordering by constructing the gate vector in the desired sequence. If inter-gate dependencies emerge in the future, use priority metadata or a DAG scheduler — for M1, deterministic FIFO is sufficient.

### 3.3 Gate Report

```rust
// src/release/report.rs

pub struct GateReport {
    pub results: Vec<GateResult>,
    pub overall: bool,    // all gates passed
    pub timestamp: DateTime<Utc>,
    pub version: semver::Version,
    pub duration: Duration, // total wall-clock time for all gates
}
```

Output formats:
- JSON (`--format json`) — for CI integration
- Human-readable (`--format text`, default) — for local development

---

## 4. SemVer Gate

### 4.1 Gate Implementation

```rust
// src/release/gates/semver.rs

pub struct SemVerGate {
    baseline_ref: String,       // e.g., "v0.10.0" (git tag or previous release)
    crate_path: PathBuf,        // e.g., "crates/fusion-plugin-api"
}

impl ReleaseGate for SemVerGate {
    fn id(&self) -> GateId { GateId::Sdk1 }
    fn name(&self) -> &'static str { "SDK Compatibility (SemVer)" }
    fn description(&self) -> &'static str {
        "Verify that public API changes to fusion-plugin-api follow semver rules"
    }
    fn run(&self, ctx: &GateContext) -> Result<GateResult, GateError> {
        // 1. Build baseline: cargo semver-checks --baseline-version <ref>
        // 2. Parse JSON output
        // 3. Map to GateCheck items
        // 4. Return GateResult
    }
}
```

### 4.2 cargo semver-checks Integration

The gate shells out to `cargo semver-checks` and parses its JSON output:

```bash
cargo semver-checks check-release \
    --manifest-path crates/fusion-plugin-api/Cargo.toml \
    --baseline-version 0.10.0 \
    --format json
```

Output is mapped into the `GateResult` structure:

| semver-checks severity | GateCheck passed |
|------------------------|------------------|
| `pass`                 | `true`           |
| `info`                 | `true`           |
| `warn`                 | `true`           |
| `error`                | `false`          |
| `fatal`                | `false`          |

### 4.3 Scope

M1 enforces only `fusion-plugin-api`. Future sprints add:
- `lib.rs` public API surface
- Additional workspace crates as they stabilize

### 4.4 Backend Abstraction

```rust
trait SemVerBackend {
    fn check_release(&self, ctx: &SemVerContext) -> Result<SemVerOutput, SemVerError>;
}
```

- `CargoSemVerChecksBackend` — wraps `cargo semver-checks` (M1 default)
- (Future) `FusionApiSnapshotBackend` — custom `syn`-based analyzer

SemVerGate delegates to whichever `SemVerBackend` is configured.

---

## 5. CLI Integration

### 5.1 Implemented in M1

| Command | Description |
|---------|-------------|
| `cargo fusion gates check [--gate SDK-1]` | Run release gates |
| `cargo fusion gates list` | List all registered gates with metadata |
| `cargo fusion gates explain <GATE-ID>` | Show gate description + current status |
| `cargo fusion features list` | List feature flags with state + metadata |

### 5.2 Architecture

All commands live under `src/devex/commands/` but delegate to `fusion-core`:

```
devex/commands/gates.rs
    │   parses CLI arguments
    │   constructs GateRunner from release module
    │   calls runner
    │   renders result to terminal
    ▼
release/ module
    │   owns all gate logic
    │   GateRunner, GateContext, ReleaseGate trait
    ▼
feature_gate/ module
    │   FeatureRegistry, FeatureFlag enum
    ▼
config/ module
    │   AppConfig, ConfigManager
```

---

## 6. Testing Strategy

| Layer | What | How |
|-------|------|-----|
| FeatureRegistry | `is_enabled()`, `apply_config()`, subscribe/prepare/commit/rollback | Unit tests, no external deps |
| FeatureRegistry + ConfigManager | Live reload of feature flags | Integration test via ConfigSubscriber|
| ReleaseGate trait | `run()` returns expected `GateResult` shape | Test with mock gate + mock context |
| SemVerGate | Invokes `cargo semver-checks` and parses output | Integration test against `fusion-plugin-api` |
| CLI | `gates list`, `features list` | Integration test via command parsing |
| Feature flag compile-time bridge | `is_effectively_enabled()` with cfg gating | Unit test with different cfg combinations |

Total expected: **~25 new tests** (15 unit, 10 integration)

---

## 7. Files Changed / Created

| File | Action | Purpose |
|------|--------|---------|
| `src/feature_gate/mod.rs` | NEW | `FeatureFlag`, `FeatureDefinition`, `FeatureRegistry`, `Stability` |
| `src/feature_gate/config_subscriber.rs` | NEW | `ConfigSubscriber` impl for hot-reload |
| `src/release/mod.rs` | NEW | Re-export `gate`, `runner`, `report`, `gates` |
| `src/release/gate.rs` | NEW | `ReleaseGate` trait, `GateId`, `GateContext`, `GateResult`, `GateCheck`, `GateError` |
| `src/release/runner.rs` | NEW | `GateRunner` |
| `src/release/report.rs` | NEW | `GateReport` |
| `src/release/gates/mod.rs` | NEW | Re-export gates |
| `src/release/gates/semver.rs` | NEW | `SemVerGate`, `SemVerBackend` trait, `CargoSemVerChecksBackend` |
| `src/config/mod.rs` | EDIT | Add `features: HashMap<String, FeatureConfig>` to `AppConfig` |
| `config/default.yaml` | EDIT | Add `features:` section |
| `src/devex/commands/gates.rs` | NEW | `gates check/list/explain` subcommands |
| `src/devex/commands/features.rs` | NEW | `features list` subcommand |
| `src/devex/mod.rs` | EDIT | Register new command modules |
| `src/lib.rs` | EDIT | `pub mod feature_gate; pub mod release;` |

---

## 8. Future-Proofing

| M2 (Compatibility) | M3 (SDK Ecosystem) |
|--------------------|--------------------|
| `release/gates/replay.rs` — ReplayGate validates cross-version snapshot compatibility | `cargo fusion certify connector` — connector conformance suite |
| `release/gates/upgrade.rs` — UpgradeGate validates green/blue upgrade safety | `cargo fusion certify plugin` — plugin ABI/version validation |
| `release/gates/determinism.rs` — DeterminismGate validates compiler determinism | `cargo fusion certify strategy` — strategy contract compliance |
| Waiver system for documented exceptions | Conformance test harness |
