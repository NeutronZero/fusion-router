# Sprint M1 — Release Gate Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make release gates executable through feature-flag infrastructure, SemVer enforcement, and a pluggable gate runner.

**Architecture:** All governance logic lives in `src/` (feature_gate, release modules). CLI commands in `devex/commands/` delegate to core types. Server has no CLI dependency.

**Tech Stack:** `serde` (existing), `semver` (existing), `clap` (new), `cargo semver-checks` (external)

## Global Constraints

- All governance logic lives in `src/`. CLI in `devex/commands/` is a thin renderer.
- `FeatureFlag` is a strongly typed enum with serde kebab-case.
- `FeatureRegistry` supports two-phase hot-reload via `ConfigSubscriber` trait.
- `FeatureDefinition` uses `&'static` lifetime — definitions are always static compile-time arrays. Never construct them dynamically at runtime.
- `ReleaseGate` trait with `GateMetadata` and FIFO execution order in `GateRunner`.
- `SemVerGate` wraps `cargo semver-checks` behind a `SemVerBackend` trait. Process execution and JSON parsing are separate functions so each can be unit-tested independently.
- CLI commands use clap for argument parsing; output supports `--format json|text`.
- Tests use `#[cfg(test)] mod tests` blocks in each file plus integration tests in `tests/`.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/feature_gate/mod.rs` | `FeatureFlag` enum, `FeatureDefinition`, `Stability`, `FeatureRegistry`, `FeatureState`. Lookup map derived from definitions during `new()`. |
| `src/feature_gate/config_subscriber.rs` | `ConfigSubscriber` impl for `FeatureRegistry` hot-reload |
| `src/release/mod.rs` | Re-exports |
| `src/release/gate.rs` | `GateId` enum, `GateResult`, `GateCheck`, `GateExecution`, `GateContext`, `GateMetadata`, `GateCategory`, `ReleaseGate` trait, `GateError` |
| `src/release/runner.rs` | `GateRunner` — returns `Vec<GateExecution>` distinguishing success vs. execution failure |
| `src/release/bootstrap.rs` | `bootstrap()` function that returns `(GateRunner, FeatureRegistry)` for use by both CLI and tests |
| `src/release/report.rs` | `GateReport` with JSON/text output |
| `src/release/gates/mod.rs` | Gate implementations re-export |
| `src/release/gates/semver.rs` | `SemVerBackend` trait, `CargoSemVerChecksBackend`, `SemVerGate` |
| `src/devex/commands/mod.rs` | Re-export command modules |
| `src/devex/commands/gates.rs` | `gates check`, `gates list`, `gates explain` command logic |
| `src/devex/commands/features.rs` | `features list` command logic |
| `src/devex/mod.rs` | Register `commands` module |
| `src/config/mod.rs` | Add `features: HashMap<String, FeatureConfig>` |
| `config/default.yaml` | Add `features:` section |
| `src/lib.rs` | Add `pub mod feature_gate; pub mod release;` |
| `Cargo.toml` | Add `clap` dependency |
| `src/bin/fusion.rs` | CLI binary entry point (thin frontend) |
| `tests/release_gate_tests.rs` | Integration tests |

---

### Task 1: FeatureFlag, FeatureDefinition, FeatureRegistry

**Files:**
- Create: `src/feature_gate/mod.rs`
- Test: inline `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `FeatureFlag` enum (Streaming, Replay, ConnectorHealth, SemanticCache, WasmPlugins) with serde kebab-case; `Stability` enum (Experimental, Stable, Deprecated); `FeatureDefinition` struct with id, introduced, removed, stability, default_enabled, description; `FeatureRegistry` with new(), apply_config(), is_enabled(), is_effectively_enabled(), list()

Key design: `lookup_map: HashMap<String, FeatureFlag>` is built once during `new()` by iterating `definitions` and calling `serde_json::to_string(&def.id)`. No manual `match` needed.

- [ ] **Step 1: Write the failing tests**

```rust
// src/feature_gate/mod.rs — in tests block at bottom

#[test]
fn test_feature_flag_serde_round_trip() {
    let flag = FeatureFlag::Streaming;
    let json = serde_json::to_string(&flag).unwrap();
    assert_eq!(json, "\"streaming\"");
    let back: FeatureFlag = serde_json::from_str(&json).unwrap();
    assert_eq!(back, flag);
}

#[test]
fn test_feature_registry_defaults() {
    let definitions = &[
        FeatureDefinition {
            id: FeatureFlag::ConnectorHealth,
            introduced: semver::Version::new(0, 11, 0),
            removed: None,
            stability: Stability::Stable,
            default_enabled: true,
            description: "Periodic connector health checks",
        },
    ];
    let registry = FeatureRegistry::new(definitions);
    assert!(registry.is_enabled(FeatureFlag::ConnectorHealth));
}

#[test]
fn test_apply_config_disables_feature() {
    let definitions = &[
        FeatureDefinition {
            id: FeatureFlag::ConnectorHealth,
            introduced: semver::Version::new(0, 11, 0),
            removed: None,
            stability: Stability::Stable,
            default_enabled: true,
            description: "",
        },
    ];
    let mut registry = FeatureRegistry::new(definitions);
    let mut overrides = HashMap::new();
    overrides.insert("connector-health".to_string(), FeatureConfig { enabled: false });
    registry.apply_config(&overrides);
    assert!(!registry.is_enabled(FeatureFlag::ConnectorHealth));
}

#[test]
fn test_apply_config_unknown_feature_is_ignored() {
    let definitions = &[];
    let mut registry = FeatureRegistry::new(definitions);
    let mut overrides = HashMap::new();
    overrides.insert("nonexistent".to_string(), FeatureConfig { enabled: true });
    registry.apply_config(&overrides); // should not panic
}

#[test]
fn test_list_returns_all_features_with_state() {
    let definitions = &[
        FeatureDefinition {
            id: FeatureFlag::Streaming,
            introduced: semver::Version::new(0, 11, 0),
            removed: None,
            stability: Stability::Stable,
            default_enabled: true,
            description: "Streaming execution support",
        },
    ];
    let registry = FeatureRegistry::new(definitions);
    let list = registry.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, FeatureFlag::Streaming);
    assert_eq!(list[0].enabled, true);
    assert_eq!(list[0].overridden, false);
}

#[test]
fn test_lookup_from_definition_works() {
    let definitions = &[
        FeatureDefinition {
            id: FeatureFlag::WasmPlugins,
            introduced: semver::Version::new(0, 10, 0),
            removed: None,
            stability: Stability::Experimental,
            default_enabled: false,
            description: "WASM plugin runtime support",
        },
    ];
    let registry = FeatureRegistry::new(definitions);
    // The lookup map is derived from serde serialization, not a manual match
    registry.apply_config(&HashMap::from([
        ("wasm-plugins".into(), FeatureConfig { enabled: true }),
    ]));
    assert!(registry.is_enabled(FeatureFlag::WasmPlugins));
}
```

- [ ] **Step 2: Run test — verify failures**

```
cargo test test_feature_flag_serde_round_trip 2>&1 | Select-String "FAILED"
```

- [ ] **Step 3: Write the implementation**

```rust
// src/feature_gate/mod.rs

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeatureFlag {
    Streaming,
    Replay,
    ConnectorHealth,
    SemanticCache,
    WasmPlugins,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stability {
    Experimental,
    Stable,
    Deprecated,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct FeatureState {
    pub id: FeatureFlag,
    pub enabled: bool,
    pub overridden: bool,
    /// `'static` invariant: FeatureDefinitions are always static compile-time arrays.
    /// Never construct them dynamically at runtime.
    pub definition: &'static FeatureDefinition,
}

#[derive(Debug)]
pub struct FeatureRegistry {
    registry: HashMap<FeatureFlag, FeatureState>,
    /// Maps config string names (kebab-case, from serde) → FeatureFlag.
    /// Built once during `new()` — no manual match needed.
    lookup_map: HashMap<String, FeatureFlag>,
    definitions: &'static [FeatureDefinition],
}

impl FeatureRegistry {
    pub fn new(definitions: &'static [FeatureDefinition]) -> Self {
        let mut lookup_map = HashMap::new();
        let registry = definitions.iter().map(|def| {
            // Derive the canonical kebab-case name from serde serialization
            let name = serde_json::to_value(&def.id)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            lookup_map.insert(name, def.id);
            let state = FeatureState {
                id: def.id,
                enabled: def.default_enabled,
                overridden: false,
                definition: def,
            };
            (def.id, state)
        }).collect();
        Self { registry, lookup_map, definitions }
    }

    pub fn apply_config(&mut self, overrides: &HashMap<String, FeatureConfig>) {
        for (name, config) in overrides {
            if let Some(&flag) = self.lookup_map.get(name) {
                if let Some(state) = self.registry.get_mut(&flag) {
                    state.enabled = config.enabled;
                    state.overridden = true;
                }
            }
            // Unknown feature names are silently ignored (forward compatibility)
        }
    }

    pub fn is_enabled(&self, flag: FeatureFlag) -> bool {
        self.registry.get(&flag).map(|s| s.enabled).unwrap_or(false)
    }

    pub fn is_effectively_enabled(&self, flag: FeatureFlag) -> bool {
        if !self.compile_time_enabled(flag) { return false; }
        self.is_enabled(flag)
    }

    pub fn list(&self) -> Vec<&FeatureState> {
        self.registry.values().collect()
    }

    fn compile_time_enabled(&self, flag: FeatureFlag) -> bool {
        match flag {
            FeatureFlag::SemanticCache => cfg!(feature = "semantic-cache"),
            FeatureFlag::WasmPlugins => cfg!(feature = "wasm-plugins"),
            _ => true,
        }
    }
}
```

- [ ] **Step 4: Run tests — verify all pass**

Run: `cargo test test_feature_flag_serde_round_trip test_feature_registry_defaults test_apply_config_disables_feature test_apply_config_unknown_feature_is_ignored test_list_returns_all_features_with_state test_lookup_from_definition_works`
Expected: 6 passed

- [ ] **Step 5: Commit**

```
git add src/feature_gate/mod.rs
git commit -m "feat: add FeatureFlag, FeatureDefinition, FeatureRegistry types"
```

---

### Task 2: FeatureRegistry ConfigSubscriber

**Files:**
- Create: `src/feature_gate/config_subscriber.rs`
- Test: inline `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `FeatureRegistry`, `AppConfig` (from `crate::config`)
- Produces: `FeatureGateSubscriber` implementing `ConfigSubscriber` trait

- [ ] **Step 1: Write the failing tests**

```rust
// src/feature_gate/config_subscriber.rs — tests block

#[test]
fn test_subscriber_prepare_parses_overrides() {
    let definitions = &[];
    let registry = Arc::new(parking_lot::RwLock::new(FeatureRegistry::new(definitions)));
    let subscriber = FeatureGateSubscriber::new(registry.clone());
    let config = AppConfig::default();
    assert!(subscriber.prepare(&config).is_ok());
}

#[test]
fn test_subscriber_commit_applies_changes() {
    let def = FeatureDefinition {
        id: FeatureFlag::Streaming,
        introduced: semver::Version::new(0, 11, 0),
        removed: None,
        stability: Stability::Stable,
        default_enabled: true,
        description: "",
    };
    let registry = Arc::new(parking_lot::RwLock::new(FeatureRegistry::new(&[def])));
    let subscriber = FeatureGateSubscriber::new(registry.clone());
    let mut config = AppConfig::default();
    config.features.insert("streaming".into(), FeatureConfig { enabled: false });
    subscriber.prepare(&config).unwrap();
    subscriber.commit();
    assert!(!registry.read().is_enabled(FeatureFlag::Streaming));
}

#[test]
fn test_subscriber_rollback_discards() {
    let def = FeatureDefinition {
        id: FeatureFlag::Streaming,
        introduced: semver::Version::new(0, 11, 0),
        removed: None,
        stability: Stability::Stable,
        default_enabled: true,
        description: "",
    };
    let registry = Arc::new(parking_lot::RwLock::new(FeatureRegistry::new(&[def])));
    let subscriber = FeatureGateSubscriber::new(registry.clone());
    let mut config = AppConfig::default();
    config.features.insert("streaming".into(), FeatureConfig { enabled: false });
    subscriber.prepare(&config).unwrap();
    subscriber.rollback();
    assert!(registry.read().is_enabled(FeatureFlag::Streaming));
}
```

- [ ] **Step 2: Run test — verify failures**

```
cargo test test_subscriber_prepare_parses_overrides 2>&1 | Select-String "FAILED"
```

- [ ] **Step 3: Write the implementation**

```rust
// src/feature_gate/config_subscriber.rs

use std::sync::Arc;
use parking_lot::RwLock;
use crate::config::manager::{ConfigSubscriber, ConfigSnapshot};
use crate::config::{AppConfig, FeatureConfig};
use crate::feature_gate::FeatureRegistry;

pub struct FeatureGateSubscriber {
    registry: Arc<RwLock<FeatureRegistry>>,
    pending: parking_lot::RwLock<Option<Vec<(String, FeatureConfig)>>>,
}

impl FeatureGateSubscriber {
    pub fn new(registry: Arc<RwLock<FeatureRegistry>>) -> Self {
        Self {
            registry,
            pending: parking_lot::RwLock::new(None),
        }
    }
}

impl ConfigSubscriber for FeatureGateSubscriber {
    fn name(&self) -> &'static str {
        "feature_gate"
    }

    fn prepare(&self, config: &AppSnapshot<'_>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let overrides: Vec<(String, FeatureConfig)> = config
            .features
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        *self.pending.write() = Some(overrides);
        Ok(())
    }

    fn commit(&self) {
        if let Some(overrides) = self.pending.write().take() {
            let map: HashMap<String, FeatureConfig> = overrides.into_iter().collect();
            self.registry.write().apply_config(&map);
        }
    }

    fn rollback(&self) {
        *self.pending.write() = None;
    }
}
```

Note: `ConfigSubscriber` trait is in `src/config/manager.rs`. Read it to confirm method signatures.

- [ ] **Step 4: Run tests — verify pass**

Run: `cargo test test_subscriber_prepare_parses_overrides test_subscriber_commit_applies_changes test_subscriber_rollback_discards`
Expected: 3 passed

- [ ] **Step 5: Commit**

```
git add src/feature_gate/config_subscriber.rs
git commit -m "feat: add FeatureGateSubscriber for live reload of feature flags"
```

---

### Task 3: AppConfig features field + config/default.yaml

**Files:**
- Modify: `src/config/mod.rs` — add `features: HashMap<String, FeatureConfig>` field with `#[serde(default)]`
- Modify: `config/default.yaml` — add `features:` section

**Interfaces:**
- Consumes: `FeatureConfig` (from Task 1)
- Produces: `AppConfig.features` field for YAML deserialization

- [ ] **Step 1: Add field to AppConfig**

Add after line 31 (`pub connectors`):

```rust
    #[serde(default)]
    pub features: HashMap<String, FeatureConfig>,
```

- [ ] **Step 2: Add features section to config/default.yaml**

Add at end of file:

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
    enabled: false
```

- [ ] **Step 3: Verify compilation and config parsing**

Run: `cargo check`
Expected: clean build

Then verify config loads: `cargo test --test '*' 2>&1 | Select-String "test result"`
Expected: all existing tests pass (config parsing still works)

- [ ] **Step 4: Commit**

```
git add src/config/mod.rs config/default.yaml
git commit -m "feat: add features config field to AppConfig and default.yaml"
```

---

### Task 4A: ReleaseGate primitives (GateId, GateResult, GateExecution, types, trait)

**Files:**
- Create: `src/release/mod.rs`
- Create: `src/release/gate.rs`
- Test: inline `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `GateId` enum with `as_str()`, `from_str()`, Display, Serialize, Deserialize; `GateResult`, `GateCheck`, `GateExecution`, `GateContext`, `GateMetadata`, `GateCategory`; `ReleaseGate` trait; `GateError`

`GateExecution` distinguishes success from execution error:
```rust
pub enum GateExecution {
    Success(GateResult),
    ExecutionError(GateError),
}
```

- [ ] **Step 1: Write the failing tests**

```rust
// src/release/gate.rs — tests block

#[test]
fn test_gate_id_display_and_parse() {
    assert_eq!(GateId::Sdk1.to_string(), "SDK-1");
    assert_eq!(GateId::from_str("RPL-1"), Some(GateId::Replay1));
    assert_eq!(GateId::from_str("UNKNOWN"), None);
}

#[test]
fn test_gate_id_serde() {
    let json = serde_json::to_string(&GateId::Sdk1).unwrap();
    assert_eq!(json, "\"SDK-1\"");
    let back: GateId = serde_json::from_str(&json).unwrap();
    assert_eq!(back, GateId::Sdk1);
}

#[test]
fn test_gate_metadata_has_required_fields() {
    let meta = GateMetadata {
        id: GateId::Sdk1,
        category: GateCategory::Compatibility,
        required: true,
        introduced: semver::Version::new(0, 11, 0),
    };
    assert_eq!(meta.id, GateId::Sdk1);
    assert!(meta.required);
}

#[test]
fn test_gate_result_passed_serde() {
    let result = GateResult {
        gate_id: GateId::Sdk1,
        passed: true,
        summary: "All checks passed".into(),
        details: vec![GateCheck {
            name: "api-compat".into(),
            passed: true,
            message: "No breaking changes".into(),
        }],
        duration: std::time::Duration::from_secs(1),
    };
    let json = serde_json::to_string(&result).unwrap();
    let back: GateResult = serde_json::from_str(&json).unwrap();
    assert!(back.passed);
    assert_eq!(back.gate_id, GateId::Sdk1);
}

#[test]
fn test_gate_execution_success() {
    let result = GateResult {
        gate_id: GateId::Sdk1,
        passed: true,
        summary: "ok".into(),
        details: vec![],
        duration: std::time::Duration::default(),
    };
    let exec = GateExecution::Success(result);
    assert!(exec.passed());
    assert!(!exec.is_error());
}

#[test]
fn test_gate_execution_error() {
    let exec = GateExecution::ExecutionError(GateError::ToolNotAvailable("test".into()));
    assert!(!exec.passed());
    assert!(exec.is_error());
}
```

- [ ] **Step 2: Write the implementation**

```rust
// src/release/mod.rs
pub mod gate;
pub mod runner;
pub mod report;
pub mod bootstrap;
pub mod gates;
```

```rust
// src/release/gate.rs

use std::fmt;
use std::time::Duration;
use serde::{Deserialize, Serialize};

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
    pub workspace_root: std::path::PathBuf,
    pub baseline_version: Option<semver::Version>,
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

#[derive(Debug, thiserror::Error)]
pub enum GateError {
    #[error("gate execution failed: {0}")]
    ExecutionFailed(String),
    #[error("external tool not available: {0}")]
    ToolNotAvailable(String),
}

/// Distinguishes a gate that ran and produced a result (even if it failed)
/// from a gate that could not execute at all.
#[derive(Debug)]
pub enum GateExecution {
    Success(GateResult),
    ExecutionError(GateError),
}

impl GateExecution {
    pub fn passed(&self) -> bool {
        match self {
            GateExecution::Success(r) => r.passed,
            GateExecution::ExecutionError(_) => false,
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, GateExecution::ExecutionError(_))
    }
}

pub trait ReleaseGate: Send + Sync {
    fn id(&self) -> GateId;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn metadata(&self) -> GateMetadata;
    fn run(&self, ctx: &GateContext) -> Result<GateResult, GateError>;
}

#[cfg(test)]
pub struct MockGate;

#[cfg(test)]
impl ReleaseGate for MockGate {
    fn id(&self) -> GateId { GateId::Sdk1 }
    fn name(&self) -> &'static str { "Mock" }
    fn description(&self) -> &'static str { "Mock gate for testing" }
    fn metadata(&self) -> GateMetadata {
        GateMetadata {
            id: GateId::Sdk1,
            category: GateCategory::Compatibility,
            required: true,
            introduced: semver::Version::new(0, 11, 0),
        }
    }
    fn run(&self, _ctx: &GateContext) -> Result<GateResult, GateError> {
        Ok(GateResult {
            gate_id: GateId::Sdk1,
            passed: true,
            summary: "mock ok".into(),
            details: vec![],
            duration: std::time::Duration::from_secs(0),
        })
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test test_gate_id_display_and_parse test_gate_id_serde test_gate_metadata_has_required_fields test_gate_result_passed_serde test_gate_execution_success test_gate_execution_error`
Expected: 6 passed

- [ ] **Step 4: Commit**

```
git add src/release/mod.rs src/release/gate.rs
git commit -m "feat: add ReleaseGate primitives with GateExecution enum"
```

---

### Task 4B: GateRunner (with GateExecution return type + FIFO test)

**Files:**
- Create: `src/release/runner.rs`
- Test: inline `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `MockGate` (from gate.rs), `GateId`, `GateContext`, `GateResult`, `GateError`, `GateExecution`, `ReleaseGate` trait
- Produces: `GateRunner` with `run_all()` returning `Vec<GateExecution>`, `run_one()` returning `Option<GateExecution>`, `gates()` accessor

- [ ] **Step 1: Write the failing tests**

```rust
// src/release/runner.rs — tests block

#[test]
fn test_runner_run_all_returns_results() {
    let runner = GateRunner::new(vec![Box::new(MockGate)]);
    let ctx = GateContext {
        workspace_root: std::path::PathBuf::from("."),
        baseline_version: None,
    };
    let results = runner.run_all(&ctx);
    assert_eq!(results.len(), 1);
    assert!(results[0].passed());
}

#[test]
fn test_runner_run_one_by_id() {
    let runner = GateRunner::new(vec![Box::new(MockGate)]);
    let ctx = GateContext {
        workspace_root: std::path::PathBuf::from("."),
        baseline_version: None,
    };
    let result = runner.run_one(GateId::Sdk1, &ctx);
    assert!(result.is_some());
    assert!(result.unwrap().passed());
}

#[test]
fn test_runner_run_one_unknown_returns_none() {
    let runner = GateRunner::new(vec![Box::new(MockGate)]);
    let ctx = GateContext {
        workspace_root: std::path::PathBuf::from("."),
        baseline_version: None,
    };
    assert!(runner.run_one(GateId::Replay1, &ctx).is_none());
}

#[test]
fn test_runner_execution_error_preserved() {
    struct FailingGate;
    impl ReleaseGate for FailingGate {
        fn id(&self) -> GateId { GateId::Determinism1 }
        fn name(&self) -> &'static str { "Failing" }
        fn description(&self) -> &'static str { "" }
        fn metadata(&self) -> GateMetadata {
            GateMetadata {
                id: GateId::Determinism1,
                category: GateCategory::Determinism,
                required: false,
                introduced: semver::Version::new(0, 11, 0),
            }
        }
        fn run(&self, _ctx: &GateContext) -> Result<GateResult, GateError> {
            Err(GateError::ToolNotAvailable("missing-tool".into()))
        }
    }
    let runner = GateRunner::new(vec![Box::new(FailingGate)]);
    let ctx = GateContext {
        workspace_root: std::path::PathBuf::from("."),
        baseline_version: None,
    };
    let results = runner.run_all(&ctx);
    assert_eq!(results.len(), 1);
    assert!(results[0].is_error());  // Execution error, not a failed result
}

#[test]
fn test_runner_fifo_order() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct OrderedGate(usize, &'static AtomicUsize);
    impl ReleaseGate for OrderedGate {
        fn id(&self) -> GateId {
            match self.0 { 0 => GateId::Sdk1, 1 => GateId::Replay1, _ => GateId::Determinism1 }
        }
        fn name(&self) -> &'static str { "Ordered" }
        fn description(&self) -> &'static str { "" }
        fn metadata(&self) -> GateMetadata {
            GateMetadata { id: self.id(), category: GateCategory::Compatibility, required: false, introduced: semver::Version::new(0, 11, 0) }
        }
        fn run(&self, _ctx: &GateContext) -> Result<GateResult, GateError> {
            self.1.store(self.0, Ordering::SeqCst);
            Ok(GateResult {
                gate_id: self.id(),
                passed: true,
                summary: format!("gate {}", self.0),
                details: vec![],
                duration: std::time::Duration::default(),
            })
        }
    }

    let order = AtomicUsize::new(99);
    let runner = GateRunner::new(vec![
        Box::new(OrderedGate(0, &order)),
        Box::new(OrderedGate(1, &order)),
        Box::new(OrderedGate(2, &order)),
    ]);
    let ctx = GateContext {
        workspace_root: std::path::PathBuf::from("."),
        baseline_version: None,
    };
    runner.run_all(&ctx);
    // The last gate to run (gate 2) should have written 2
    assert_eq!(order.load(Ordering::SeqCst), 2);
}
```

- [ ] **Step 2: Write the implementation**

```rust
// src/release/runner.rs

use crate::release::gate::{GateId, GateContext, GateExecution, ReleaseGate};

pub struct GateRunner {
    gates: Vec<Box<dyn ReleaseGate>>,
}

impl GateRunner {
    pub fn new(gates: Vec<Box<dyn ReleaseGate>>) -> Self {
        Self { gates }
    }

    pub fn register(&mut self, gate: Box<dyn ReleaseGate>) {
        self.gates.push(gate);
    }

    /// Runs every gate in registration order (FIFO). Returns a `GateExecution`
    /// per gate so callers can distinguish success from execution errors.
    pub fn run_all(&self, ctx: &GateContext) -> Vec<GateExecution> {
        self.gates.iter().map(|gate| {
            match gate.run(ctx) {
                Ok(result) => GateExecution::Success(result),
                Err(e) => GateExecution::ExecutionError(e),
            }
        }).collect()
    }

    pub fn run_one(&self, id: GateId, ctx: &GateContext) -> Option<GateExecution> {
        self.gates.iter().find(|g| g.id() == id).map(|gate| {
            match gate.run(ctx) {
                Ok(result) => GateExecution::Success(result),
                Err(e) => GateExecution::ExecutionError(e),
            }
        })
    }

    pub fn gates(&self) -> &[Box<dyn ReleaseGate>] {
        &self.gates
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test test_runner_run_all_returns_results test_runner_run_one_by_id test_runner_run_one_unknown_returns_none test_runner_execution_error_preserved test_runner_fifo_order`
Expected: 5 passed

- [ ] **Step 4: Commit**

```
git add src/release/runner.rs
git commit -m "feat: add GateRunner with GateExecution return type and FIFO ordering"
```

---

### Task 4C: GateReport

**Files:**
- Create: `src/release/report.rs`
- Test: inline `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `GateResult` (from gate.rs)
- Produces: `GateReport` with `new()`, `to_json()`, `to_text()`

- [ ] **Step 1: Write the failing tests**

```rust
// src/release/report.rs — tests block

#[test]
fn test_report_overall_all_pass() {
    let results = vec![
        GateResult {
            gate_id: GateId::Sdk1,
            passed: true,
            summary: "ok".into(),
            details: vec![],
            duration: std::time::Duration::from_secs(0),
        },
    ];
    let report = GateReport::new(results, semver::Version::new(0, 11, 0));
    assert!(report.overall);
}

#[test]
fn test_report_overall_any_fail() {
    let results = vec![
        GateResult {
            gate_id: GateId::Sdk1,
            passed: true,
            summary: "ok".into(),
            details: vec![],
            duration: std::time::Duration::from_secs(0),
        },
        GateResult {
            gate_id: GateId::Replay1,
            passed: false,
            summary: "failed".into(),
            details: vec![],
            duration: std::time::Duration::from_secs(0),
        },
    ];
    let report = GateReport::new(results, semver::Version::new(0, 11, 0));
    assert!(!report.overall);
}

#[test]
fn test_report_to_json_contains_overall() {
    let report = GateReport::new(vec![], semver::Version::new(0, 11, 0));
    let json = report.to_json();
    assert!(json.contains("overall"));
}

#[test]
fn test_report_to_text_shows_pass_fail() {
    let results = vec![
        GateResult {
            gate_id: GateId::Sdk1,
            passed: true,
            summary: "ok".into(),
            details: vec![],
            duration: std::time::Duration::from_secs(0),
        },
    ];
    let report = GateReport::new(results, semver::Version::new(0, 11, 0));
    let text = report.to_text();
    assert!(text.contains("PASS"));
}
```

- [ ] **Step 2: Write the implementation**

```rust
// src/release/report.rs

use std::time::Duration;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::release::gate::GateResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateReport {
    pub results: Vec<GateResult>,
    pub overall: bool,
    pub timestamp: DateTime<Utc>,
    pub version: semver::Version,
    pub duration: Duration,
}

impl GateReport {
    pub fn new(results: Vec<GateResult>, version: semver::Version) -> Self {
        let overall = results.iter().all(|r| r.passed);
        let duration = results.iter().map(|r| r.duration).sum();
        Self {
            results,
            overall,
            timestamp: Utc::now(),
            version,
            duration,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap()
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Release Gate Report — v{}\n", self.version));
        out.push_str(&format!("Timestamp: {}\n", self.timestamp));
        out.push_str(&format!("Overall: {}\n\n", if self.overall { "PASS" } else { "FAIL" }));
        for result in &self.results {
            let status = if result.passed { "PASS" } else { "FAIL" };
            out.push_str(&format!("{status} | {} | {}\n", result.gate_id, result.summary));
            for check in &result.details {
                let c_status = if check.passed { "  ✓" } else { "  ✗" };
                out.push_str(&format!("{c_status} {} — {}\n", check.name, check.message));
            }
        }
        out.push_str(&format!("\nDuration: {:?}", self.duration));
        out
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test test_report_overall_all_pass test_report_overall_any_fail test_report_to_json_contains_overall test_report_to_text_shows_pass_fail`
Expected: 4 passed

- [ ] **Step 4: Run all release tests**

Run: `cargo test -p fusion-router -- release 2>&1`
Expected: all tests pass

- [ ] **Step 5: Commit**

```
git add src/release/report.rs
git commit -m "feat: add GateReport with JSON and text output"
```

---

### Task 5: SemVer Backend + Gate

**Files:**
- Create: `src/release/gates/mod.rs`
- Create: `src/release/gates/semver.rs`
- Test: inline `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `GateId::Sdk1`, `ReleaseGate` trait, `GateResult`, `GateContext`, `GateMetadata`, `GateCategory`
- Produces: `SemVerBackend` trait, `CargoSemVerChecksBackend`, `SemVerGate`, standalone `parse_semver_checks_output()` (testable without spawning a process)

- [ ] **Step 1: Write failing tests** (includes parser tests separate from process)

```rust
// src/release/gates/semver.rs — tests block

#[test]
fn test_semver_gate_metadata() {
    let gate = SemVerGate::new("v0.10.0", "crates/fusion-plugin-api");
    let meta = gate.metadata();
    assert_eq!(meta.id, GateId::Sdk1);
    assert_eq!(meta.category, GateCategory::Compatibility);
    assert!(meta.required);
}

#[test]
fn test_mock_backend_returns_pass() {
    let gate = SemVerGate::with_backend(
        "v0.10.0",
        "crates/fusion-plugin-api",
        MockBackend { should_pass: true },
    );
    let ctx = GateContext {
        workspace_root: std::path::PathBuf::from("."),
        baseline_version: Some(semver::Version::new(0, 10, 0)),
    };
    let result = gate.run(&ctx).unwrap();
    assert!(result.passed);
}

#[test]
fn test_mock_backend_returns_fail() {
    let gate = SemVerGate::with_backend(
        "v0.10.0",
        "crates/fusion-plugin-api",
        MockBackend { should_pass: false },
    );
    let ctx = GateContext {
        workspace_root: std::path::PathBuf::from("."),
        baseline_version: Some(semver::Version::new(0, 10, 0)),
    };
    let result = gate.run(&ctx).unwrap();
    assert!(!result.passed);
}

// Parser tests — no process spawning needed
#[test]
fn test_parse_semver_output_all_pass() {
    let json = r#"{"checks":[{"severity":"pass","message":"ok","name":"check1"}]}"#;
    let checks = parse_semver_checks_output(json).unwrap();
    assert_eq!(checks.len(), 1);
    assert!(checks[0].passed);
}

#[test]
fn test_parse_semver_output_with_errors() {
    let json = r#"{"checks":[
        {"severity":"pass","message":"ok","name":"check1"},
        {"severity":"error","message":"breaking change","name":"check2"}
    ]}"#;
    let checks = parse_semver_checks_output(json).unwrap();
    assert_eq!(checks.len(), 2);
    assert!(checks[0].passed);
    assert!(!checks[1].passed);
}

#[test]
fn test_parse_semver_output_invalid_json() {
    let result = parse_semver_checks_output("not json");
    assert!(result.is_err());
}
```

- [ ] **Step 2: Write implementation**

```rust
// src/release/gates/mod.rs
pub mod semver;
```

```rust
// src/release/gates/semver.rs

use std::path::PathBuf;
use std::time::Instant;
use crate::release::gate::{
    GateId, GateResult, GateCheck, GateContext, GateError, GateMetadata,
    GateCategory, ReleaseGate,
};

pub trait SemVerBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn check_release(&self, crate_path: &PathBuf, baseline_ref: &str) -> Result<Vec<GateCheck>, GateError>;
}

pub struct CargoSemVerChecksBackend;

impl CargoSemVerChecksBackend {
    pub fn new() -> Self { Self }
}

impl SemVerBackend for CargoSemVerChecksBackend {
    fn name(&self) -> &'static str { "cargo-semver-checks" }

    fn check_release(&self, crate_path: &PathBuf, baseline_ref: &str) -> Result<Vec<GateCheck>, GateError> {
        let manifest = crate_path.join("Cargo.toml");
        let output = std::process::Command::new("cargo")
            .args([
                "semver-checks", "check-release",
                "--manifest-path", manifest.to_str().unwrap_or(""),
                "--baseline-version", baseline_ref,
                "--format", "json",
            ])
            .output()
            .map_err(|e| GateError::ToolNotAvailable(format!("cargo semver-checks: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GateError::ExecutionFailed(format!(
                "cargo semver-checks failed: {stderr}"
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_semver_checks_output(&stdout)
    }
}

/// Standalone parser — testable without spawning cargo semver-checks.
pub fn parse_semver_checks_output(output: &str) -> Result<Vec<GateCheck>, GateError> {
    #[derive(serde::Deserialize)]
    struct SemVerCheck {
        severity: String,
        #[serde(default)]
        message: String,
        #[serde(default)]
        name: String,
    }

    #[derive(serde::Deserialize)]
    struct SemVerOutput {
        #[serde(default)]
        checks: Vec<SemVerCheck>,
    }

    let parsed: SemVerOutput = serde_json::from_str(output)
        .map_err(|e| GateError::ExecutionFailed(format!("parse semver output: {e}")))?;

    Ok(parsed.checks.into_iter().map(|c| GateCheck {
        name: if c.name.is_empty() { "check".into() } else { c.name },
        passed: matches!(c.severity.as_str(), "pass" | "info" | "warn"),
        message: c.message,
    }).collect())
}

pub struct SemVerGate {
    baseline_ref: String,
    crate_path: PathBuf,
    backend: Box<dyn SemVerBackend>,
}

impl SemVerGate {
    pub fn new(baseline_ref: &str, crate_path: &str) -> Self {
        Self {
            baseline_ref: baseline_ref.to_string(),
            crate_path: PathBuf::from(crate_path),
            backend: Box::new(CargoSemVerChecksBackend::new()),
        }
    }

    pub fn with_backend(baseline_ref: &str, crate_path: &str, backend: impl SemVerBackend + 'static) -> Self {
        Self {
            baseline_ref: baseline_ref.to_string(),
            crate_path: PathBuf::from(crate_path),
            backend: Box::new(backend),
        }
    }
}

impl ReleaseGate for SemVerGate {
    fn id(&self) -> GateId { GateId::Sdk1 }
    fn name(&self) -> &'static str { "SDK Compatibility (SemVer)" }
    fn description(&self) -> &'static str {
        "Verify that public API changes to fusion-plugin-api follow semver rules"
    }
    fn metadata(&self) -> GateMetadata {
        GateMetadata {
            id: GateId::Sdk1,
            category: GateCategory::Compatibility,
            required: true,
            introduced: semver::Version::new(0, 11, 0),
        }
    }
    fn run(&self, _ctx: &GateContext) -> Result<GateResult, GateError> {
        let start = Instant::now();
        let checks = self.backend.check_release(&self.crate_path, &self.baseline_ref)?;
        let passed = checks.iter().all(|c| c.passed);
        let summary = if passed {
            format!("{} compatibility checks passed", checks.len())
        } else {
            let failed = checks.iter().filter(|c| !c.passed).count();
            format!("{failed} compatibility checks failed")
        };
        Ok(GateResult {
            gate_id: GateId::Sdk1,
            passed,
            summary,
            details: checks,
            duration: start.elapsed(),
        })
    }
}

// Mock backend for testing
#[cfg(test)]
pub struct MockBackend {
    pub should_pass: bool,
}

#[cfg(test)]
impl SemVerBackend for MockBackend {
    fn name(&self) -> &'static str { "mock" }
    fn check_release(&self, _crate_path: &PathBuf, _baseline_ref: &str) -> Result<Vec<GateCheck>, GateError> {
        if self.should_pass {
            Ok(vec![GateCheck {
                name: "api-compat".into(),
                passed: true,
                message: "No breaking changes".into(),
            }])
        } else {
            Ok(vec![GateCheck {
                name: "api-compat".into(),
                passed: false,
                message: "Breaking change detected".into(),
            }])
        }
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib release::gates::semver::tests 2>&1`
Expected: 6 tests pass (metadata, mock pass, mock fail, parser all pass, parser with errors, parser invalid)

- [ ] **Step 4: Commit**

```
git add src/release/gates/mod.rs src/release/gates/semver.rs
git commit -m "feat: add SemVerGate with cargo semver-checks backend"
```

---

### Task 6: Bootstrap module + CLI commands

**Files:**
- Create: `src/release/bootstrap.rs`
- Create: `src/devex/commands/mod.rs`
- Create: `src/devex/commands/gates.rs`
- Create: `src/devex/commands/features.rs`
- Modify: `src/devex/mod.rs` — register commands module

**Interfaces:**
- `release::bootstrap::bootstrap()` → `(GateRunner, FeatureRegistry)` — shared between CLI and tests
- CLI commands consume `GateRunner`, `GateReport`, `FeatureRegistry`, `GateId`, `GateExecution`

- [ ] **Step 1: Write bootstrap module**

```rust
// src/release/bootstrap.rs

use crate::feature_gate::*;
use crate::release::gate::*;
use crate::release::runner::GateRunner;
use crate::release::gates::semver::SemVerGate;

pub fn bootstrap() -> (GateRunner, FeatureRegistry) {
    let workspace = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let sdk_crate = workspace.join("crates/fusion-plugin-api");
    let baseline = std::env::var("FUSION_BASELINE_VERSION").unwrap_or_else(|_| "0.10.0".into());

    let runner = GateRunner::new(vec![
        Box::new(SemVerGate::new(&baseline, sdk_crate.to_str().unwrap_or("crates/fusion-plugin-api"))),
    ]);

    let definitions: &[FeatureDefinition] = &[
        FeatureDefinition {
            id: FeatureFlag::Streaming, introduced: semver::Version::new(0, 11, 0), removed: None,
            stability: Stability::Stable, default_enabled: true,
            description: "Streaming execution support",
        },
        FeatureDefinition {
            id: FeatureFlag::Replay, introduced: semver::Version::new(0, 11, 0), removed: None,
            stability: Stability::Stable, default_enabled: true,
            description: "Session replay support",
        },
        FeatureDefinition {
            id: FeatureFlag::ConnectorHealth, introduced: semver::Version::new(0, 11, 0), removed: None,
            stability: Stability::Stable, default_enabled: true,
            description: "Periodic connector health checks",
        },
        FeatureDefinition {
            id: FeatureFlag::SemanticCache, introduced: semver::Version::new(0, 10, 0), removed: None,
            stability: Stability::Stable, default_enabled: true,
            description: "Semantic caching for provider responses",
        },
        FeatureDefinition {
            id: FeatureFlag::WasmPlugins, introduced: semver::Version::new(0, 10, 0), removed: None,
            stability: Stability::Experimental, default_enabled: false,
            description: "WASM plugin runtime support",
        },
    ];
    let registry = FeatureRegistry::new(definitions);
    (runner, registry)
}
```

- [ ] **Step 2: Write CLI command modules**

```rust
// src/devex/commands/mod.rs
pub mod gates;
pub mod features;
```

```rust
// src/devex/commands/gates.rs

use crate::release::gate::{GateId, GateContext, ReleaseGate, GateExecution};
use crate::release::runner::GateRunner;
use crate::release::report::GateReport;
use crate::release::gate::GateResult;

pub fn list_gates(runner: &GateRunner) -> String {
    let mut out = String::from("Release Gates:\n");
    for gate in runner.gates() {
        let meta = gate.metadata();
        let required = if meta.required { "required" } else { "optional" };
        out.push_str(&format!(
            "  {} | {} | {} | {}\n",
            gate.id(),
            gate.name(),
            required,
            gate.description(),
        ));
    }
    out
}

pub fn explain_gate(runner: &GateRunner, id: &str) -> Option<String> {
    let gate_id = GateId::from_str(id)?;
    let gate = runner.gates().iter().find(|g| g.id() == gate_id)?;
    let meta = gate.metadata();
    Some(format!(
        "Gate: {}\nName: {}\nCategory: {:?}\nRequired: {}\nIntroduced: v{}\n\n{}",
        gate.id(),
        gate.name(),
        meta.category,
        meta.required,
        meta.introduced,
        gate.description(),
    ))
}

pub fn check_gates(runner: &GateRunner, ctx: &GateContext, gate_filter: Option<&str>) -> GateReport {
    let results = match gate_filter {
        Some(id_str) => {
            if let Some(id) = GateId::from_str(id_str) {
                match runner.run_one(id, ctx) {
                    Some(GateExecution::Success(r)) => vec![r],
                    Some(GateExecution::ExecutionError(e)) => {
                        vec![GateResult {
                            gate_id: id,
                            passed: false,
                            summary: format!("gate execution error: {e}"),
                            details: vec![],
                            duration: std::time::Duration::default(),
                        }]
                    }
                    None => vec![],
                }
            } else {
                Vec::new()
            }
        }
        None => {
            runner.run_all(ctx).into_iter().filter_map(|exec| {
                match exec {
                    GateExecution::Success(r) => Some(r),
                    GateExecution::ExecutionError(e) => Some(GateResult {
                        gate_id: GateId::Sdk1,
                        passed: false,
                        summary: format!("gate execution error: {e}"),
                        details: vec![],
                        duration: std::time::Duration::default(),
                    }),
                }
            }).collect()
        }
    };
    let report_version = ctx.baseline_version.clone().unwrap_or_else(|| semver::Version::new(0, 0, 0));
    GateReport::new(results, report_version)
}
```

```rust
// src/devex/commands/features.rs

use crate::feature_gate::FeatureRegistry;

pub fn list_features(registry: &FeatureRegistry) -> String {
    let mut out = String::from("Feature Flags:\n");
    for state in registry.list() {
        let status = if state.enabled { "enabled" } else { "disabled" };
        let effective = if registry.is_effectively_enabled(state.id) { "effective" } else { "ineffective" };
        let override_str = if state.overridden { " (overridden)" } else { "" };
        out.push_str(&format!(
            "  {} | {} | {}{} | v{}",
            state.definition.description,
            status,
            effective,
            override_str,
            state.definition.introduced,
        ));
        if let Some(removed) = &state.definition.removed {
            out.push_str(&format!(" | removed in v{removed}"));
        }
        out.push('\n');
    }
    out
}
```

- [ ] **Step 3: Update devex/mod.rs**

```rust
pub mod commands;
pub mod visualizer;
pub mod trace_inspector;
pub mod scaffold;

#[allow(unused_imports)]
pub use visualizer::GraphVisualizer;
#[allow(unused_imports)]
pub use trace_inspector::TraceInspector;
#[allow(unused_imports)]
pub use scaffold::PluginScaffolder;
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check`
Expected: clean build

- [ ] **Step 5: Commit**

```
git add src/release/bootstrap.rs src/devex/commands/mod.rs src/devex/commands/gates.rs src/devex/commands/features.rs src/devex/mod.rs
git commit -m "feat: add bootstrap module and CLI commands for gates and features"
```

---

### Task 7: Wire lib.rs + CLI binary + full integration

**Files:**
- Modify: `src/lib.rs` — add `pub mod feature_gate; pub mod release;`
- Modify: `Cargo.toml` — add `clap` dependency
- Create: `src/bin/fusion.rs`

- [ ] **Step 1: Wire lib.rs**

Add to `src/lib.rs` after `pub mod devex;`:

```rust
pub mod feature_gate;
pub mod release;
```

- [ ] **Step 2: Add clap to Cargo.toml**

Add after existing dependencies:

```toml
clap = { version = "4", features = ["derive"] }
```

 - [ ] **Step 3: Create CLI binary (thin frontend using bootstrap)**

```rust
// src/bin/fusion.rs

use clap::{Parser, Subcommand};
use fusion_router::release::bootstrap;
use fusion_router::release::gate::*;
use fusion_router::devex::commands;
use fusion_router::feature_gate::*;

/// FusionRouter release & development toolkit
#[derive(Parser)]
#[command(name = "fusion", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage release gates
    Gates {
        #[command(subcommand)]
        action: GatesAction,
    },
    /// Manage feature flags
    Features {
        #[command(subcommand)]
        action: FeaturesAction,
    },
}

#[derive(Subcommand)]
enum GatesAction {
    /// Run all release gates (or a specific gate by ID)
    Check {
        /// Gate ID to check (e.g., SDK-1)
        #[arg(long)]
        gate: Option<String>,
        /// Baseline version for compatibility checks
        #[arg(long, default_value = "0.10.0")]
        baseline: String,
        /// Output format: text or json
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List all registered release gates
    List,
    /// Show detailed information about a specific gate
    Explain {
        /// Gate ID (e.g., SDK-1)
        gate: String,
    },
}

#[derive(Subcommand)]
enum FeaturesAction {
    /// List all feature flags with current state
    List {
        /// Output format: text or json
        #[arg(long, default_value = "text")]
        format: String,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Gates { action } => handle_gates(action),
        Commands::Features { action } => handle_features(action),
    }
}

fn handle_gates(action: GatesAction) {
    let (runner, _registry) = bootstrap::bootstrap();

    match action {
        GatesAction::Check { gate, baseline, format } => {
            let ctx = GateContext {
                workspace_root: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
                baseline_version: Some(semver::Version::parse(&baseline).unwrap_or_else(|_| semver::Version::new(0, 10, 0))),
            };
            let report = commands::gates::check_gates(&runner, &ctx, gate.as_deref());
            match format.as_str() {
                "json" => println!("{}", report.to_json()),
                _ => println!("{}", report.to_text()),
            }
        }
        GatesAction::List => {
            print!("{}", commands::gates::list_gates(&runner));
        }
        GatesAction::Explain { gate } => {
            match commands::gates::explain_gate(&runner, &gate) {
                Some(explanation) => print!("{explanation}"),
                None => eprintln!("Unknown gate: {gate}"),
            }
        }
    }
}

fn handle_features(action: FeaturesAction) {
    let (_runner, registry) = bootstrap::bootstrap();

    match action {
        FeaturesAction::List { format } => {
            match format.as_str() {
                "json" => {
                    let list: Vec<_> = registry.list().iter().map(|s| {
                        serde_json::json!({
                            "id": s.id,
                            "enabled": s.enabled,
                            "overridden": s.overridden,
                            "introduced": s.definition.introduced.to_string(),
                            "stability": s.definition.stability,
                        })
                    }).collect();
                    println!("{}", serde_json::to_string_pretty(&list).unwrap());
                }
                _ => {
                    print!("{}", commands::features::list_features(&registry));
                }
            }
        }
    }
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check`
Expected: clean build

- [ ] **Step 5: Run CLI help**

Run: `cargo run --bin fusion -- --help`
Expected: help text displayed

- [ ] **Step 6: Run features list**

Run: `cargo run --bin fusion -- features list`
Expected: feature flags listed with status

- [ ] **Step 7: Run gates list**

Run: `cargo run --bin fusion -- gates list`
Expected: gates listed

- [ ] **Step 8: Commit**

```
git add src/lib.rs Cargo.toml src/bin/fusion.rs
git commit -m "feat: wire lib.rs exports and create fusion CLI binary"
```

---

### Task 8: Integration tests

**Files:**
- Create: `tests/release_gate_tests.rs`

- [ ] **Step 1: Write integration tests**

```rust
// tests/release_gate_tests.rs

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use fusion_router::release::gate::*;
use fusion_router::release::runner::GateRunner;
use fusion_router::release::report::GateReport;
use fusion_router::release::gates::semver::{SemVerGate, MockBackend};

fn collect_results(results: Vec<GateExecution>) -> Vec<GateResult> {
    results.into_iter().filter_map(|e| match e {
        GateExecution::Success(r) => Some(r),
        _ => None,
    }).collect()
}

#[test]
fn test_gate_runner_with_mock_semver_passing() {
    let gate = SemVerGate::with_backend(
        "v0.10.0",
        "crates/fusion-plugin-api",
        MockBackend { should_pass: true },
    );
    let runner = GateRunner::new(vec![Box::new(gate)]);
    let ctx = GateContext {
        workspace_root: PathBuf::from("."),
        baseline_version: Some(semver::Version::new(0, 10, 0)),
    };
    let report = GateReport::new(collect_results(runner.run_all(&ctx)), semver::Version::new(0, 11, 0));
    assert!(report.overall);
    assert_eq!(report.results.len(), 1);
}

#[test]
fn test_gate_runner_with_mock_semver_failing() {
    let gate = SemVerGate::with_backend(
        "v0.10.0",
        "crates/fusion-plugin-api",
        MockBackend { should_pass: false },
    );
    let runner = GateRunner::new(vec![Box::new(gate)]);
    let ctx = GateContext {
        workspace_root: PathBuf::from("."),
        baseline_version: Some(semver::Version::new(0, 10, 0)),
    };
    let report = GateReport::new(collect_results(runner.run_all(&ctx)), semver::Version::new(0, 11, 0));
    assert!(!report.overall);
}

#[test]
fn test_report_json_round_trip() {
    let gate = SemVerGate::with_backend(
        "v0.10.0",
        "crates/fusion-plugin-api",
        MockBackend { should_pass: true },
    );
    let runner = GateRunner::new(vec![Box::new(gate)]);
    let ctx = GateContext {
        workspace_root: PathBuf::from("."),
        baseline_version: Some(semver::Version::new(0, 10, 0)),
    };
    let report = GateReport::new(collect_results(runner.run_all(&ctx)), semver::Version::new(0, 11, 0));
    let json = report.to_json();
    let deserialized: GateReport = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.overall, report.overall);
    assert_eq!(deserialized.results.len(), report.results.len());
}

#[test]
fn test_fifo_execution_order() {
    struct OrderedGate(usize, &'static AtomicUsize);
    impl ReleaseGate for OrderedGate {
        fn id(&self) -> GateId { match self.0 { 0 => GateId::Sdk1, 1 => GateId::Replay1, _ => GateId::Determinism1 } }
        fn name(&self) -> &'static str { "Ordered" }
        fn description(&self) -> &'static str { "" }
        fn metadata(&self) -> GateMetadata {
            GateMetadata { id: self.id(), category: GateCategory::Compatibility, required: false, introduced: semver::Version::new(0, 11, 0) }
        }
        fn run(&self, _ctx: &GateContext) -> Result<GateResult, GateError> {
            self.1.store(self.0, Ordering::SeqCst);
            Ok(GateResult { gate_id: self.id(), passed: true, summary: format!("gate {}", self.0), details: vec![], duration: std::time::Duration::default() })
        }
    }

    let order = AtomicUsize::new(99);
    let runner = GateRunner::new(vec![
        Box::new(OrderedGate(0, &order)),
        Box::new(OrderedGate(1, &order)),
        Box::new(OrderedGate(2, &order)),
    ]);
    let ctx = GateContext { workspace_root: PathBuf::from("."), baseline_version: None };
    runner.run_all(&ctx);
    assert_eq!(order.load(Ordering::SeqCst), 2);
}

#[test]
fn test_feature_registry_integration() {
    use fusion_router::feature_gate::*;
    use std::collections::HashMap;

    let definitions = &[
        FeatureDefinition {
            id: FeatureFlag::Streaming,
            introduced: semver::Version::new(0, 11, 0),
            removed: None,
            stability: Stability::Stable,
            default_enabled: true,
            description: "Streaming execution support",
        },
    ];
    let mut registry = FeatureRegistry::new(definitions);
    assert!(registry.is_enabled(FeatureFlag::Streaming));
    assert!(registry.is_effectively_enabled(FeatureFlag::Streaming));

    let mut overrides = HashMap::new();
    overrides.insert("streaming".to_string(), FeatureConfig { enabled: false });
    registry.apply_config(&overrides);
    assert!(!registry.is_enabled(FeatureFlag::Streaming));
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test --test release_gate_tests`
Expected: 5 tests pass

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: all existing tests + new tests pass

- [ ] **Step 4: Commit**

```
git add tests/release_gate_tests.rs
git commit -m "test: add integration tests for release gates and feature registry"
```
