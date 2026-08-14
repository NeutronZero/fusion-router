# Live Configuration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver Sprint 1.1 of Epic G — Live Configuration: `ConfigManager` with `ArcSwap<ConfigSnapshot>`, transactional SIGHUP reload, `ConfigSubscriber` protocol, and live provider updates via `ProviderRegistry`.

**Architecture:** `ConfigManager` owns `ArcSwap<ConfigSnapshot>`, a subscriber list, and a config path. SIGHUP triggers parse -> validate -> prepare (subscribers build candidates) -> commit (atomic swap). `ProviderRegistry` implements `ConfigSubscriber` with two-phase prepare/commit. Request-scoped components read `snapshot()` at request start.

**Tech Stack:** Rust, `arc_swap` (new dep), `tokio::signal::unix`, `parking_lot`, existing `AppConfig`/`ProviderRegistry`.

## Global Constraints

- No breaking changes to `AppConfig` YAML schema
- No file watcher (`notify` crate) in Sprint 1.1
- No Windows SIGHUP equivalent in Sprint 1.1
- No config file splitting (`resources/`, `policies/` fragments remain unused)
- `config/default.yaml` `providers` section becomes authoritative
- All existing tests must remain green
- `cargo check` must produce zero warnings

---

## File Map

```
Cargo.toml                          # +arc_swap dependency
src/config.rs -> src/config/mod.rs  # renamed, +submodule declarations
src/config/manager.rs               # NEW: ConfigManager, ConfigSnapshot, ConfigSubscriber
src/config/error.rs                 # NEW: error types
src/providers/mod.rs                # +make CircuitBreakingProvider public
src/providers/registry.rs           # +ConfigSubscriber impl, candidate build
src/main.rs                         # ConfigManager construction, SIGHUP task, ProviderRegistry wiring
src/server/handlers.rs              # AppState uses ConfigManager, snapshot-based to_policies()
src/types/execution_context.rs      # +config_generation: u64
config/default.yaml                 # update providers section, add comments
```

---

### Task 1: Add `arc_swap` dependency and restructure config module

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/config.rs` -> rename to `src/config/mod.rs`
- Create: `src/config/error.rs`
- Create: `src/config/manager.rs`
- Test: `cargo check`

**Interfaces:**
- Consumes: existing `AppConfig`, `config::load()`, `config::validate()`
- Produces: `src/config/mod.rs` with `pub mod error; pub mod manager;` declarations

- [ ] **Step 1: Add `arc_swap` to Cargo.toml**

Edit `Cargo.toml`, add after the `parking_lot` line:

```toml
arc_swap = "1"
```

- [ ] **Step 2: Create `src/config/error.rs`**

```rust
use std::fmt;

#[derive(Debug, Clone)]
pub enum ValidationSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct ConfigValidationError {
    pub field: String,
    pub message: String,
    pub value: Option<String>,
    pub severity: ValidationSeverity,
}

impl fmt::Display for ConfigValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}: {}", self.severity, self.field, self.message)
    }
}

#[derive(Debug)]
pub enum ReloadError {
    Parse(String),
    Validation(Vec<ConfigValidationError>),
    Subscriber { name: String, reason: String },
}

impl fmt::Display for ReloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReloadError::Parse(msg) => write!(f, "parse error: {msg}"),
            ReloadError::Validation(errors) => {
                write!(f, "validation failed ({} errors)", errors.len())
            }
            ReloadError::Subscriber { name, reason } => {
                write!(f, "subscriber '{name}' rejected: {reason}")
            }
        }
    }
}

impl std::error::Error for ReloadError {}
```

- [ ] **Step 3: Create `src/config/manager.rs`**

```rust
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use arc_swap::ArcSwap;

use super::AppConfig;
use super::error::ReloadError;

#[derive(Clone)]
pub struct ConfigSnapshot {
    pub generation: u64,
    pub config: Arc<AppConfig>,
}

pub trait ConfigSubscriber: Send + Sync {
    fn priority(&self) -> u8 { 0 }

    fn prepare(
        &self,
        old: &ConfigSnapshot,
        new: &ConfigSnapshot,
    ) -> Result<(), ReloadError>;

    fn commit(&self, generation: u64);
}

pub struct ConfigManager {
    inner: ArcSwap<ConfigSnapshot>,
    pub config_path: PathBuf,
    subscribers: Vec<Box<dyn ConfigSubscriber + Send + Sync>>,
    generation: AtomicU64,
}

impl ConfigManager {
    pub fn new(
        config_path: PathBuf,
        initial_config: AppConfig,
        subscribers: Vec<Box<dyn ConfigSubscriber + Send + Sync>>,
    ) -> Self {
        let generation = 1;
        let snapshot = ConfigSnapshot {
            generation,
            config: Arc::new(initial_config),
        };
        Self {
            inner: ArcSwap::new(Arc::new(snapshot)),
            config_path,
            subscribers,
            generation: AtomicU64::new(generation),
        }
    }

    pub fn snapshot(&self) -> ConfigSnapshot {
        (*self.inner.load()).clone()
    }

    fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub async fn reload(&self) -> Result<u64, ReloadError> {
        let content = std::fs::read_to_string(&self.config_path)
            .map_err(|e| ReloadError::Parse(e.to_string()))?;

        let new_config: AppConfig = serde_yaml::from_str(&content)
            .map_err(|e| ReloadError::Parse(e.to_string()))?;

        new_config.validate().map_err(ReloadError::Validation)?;

        let next_gen = self.next_generation();
        let old_snapshot = self.snapshot();
        let new_snapshot = ConfigSnapshot {
            generation: next_gen,
            config: Arc::new(new_config),
        };

        let mut ordered: Vec<_> = self.subscribers.iter().collect();
        ordered.sort_by_key(|s| s.priority());

        for subscriber in &ordered {
            subscriber.prepare(&old_snapshot, &new_snapshot)?;
        }

        for subscriber in &ordered {
            subscriber.commit(next_gen);
        }

        self.inner.store(Arc::new(new_snapshot));
        self.generation.store(next_gen, Ordering::SeqCst);

        tracing::info!(generation = next_gen, "configuration reloaded");
        Ok(next_gen)
    }
}
```

- [ ] **Step 4: Restructure `src/config.rs` into `src/config/mod.rs`**

Rename `src/config.rs` to `src/config/mod.rs`. Add at the top:

```rust
pub mod error;
pub mod manager;
```

Keep the existing `AppConfig`, sub-configs, `load()`, and `validate()` as-is.

- [ ] **Step 5: Run `cargo check` to verify the module restructure compiles**

Run: `cargo check`
Expected: zero warnings, zero errors

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/config.rs src/config/mod.rs src/config/error.rs src/config/manager.rs
git mv src/config.rs src/config/mod.rs
git commit -m "feat: add arc_swap, config error types, ConfigManager with transactional reload"
```

---

### Task 2: Upgrade AppConfig::validate() to return structured errors

**Files:**
- Modify: `src/config/mod.rs` (AppConfig::validate)
- Test: existing validation tests + validate with structured errors

**Interfaces:**
- Consumes: `ConfigValidationError`, `ValidationSeverity` from `error` module
- Produces: updated `validate()` signature returning `Result<(), Vec<ConfigValidationError>>`

- [ ] **Step 1: Update `AppConfig::validate()` signature**

In `src/config/mod.rs`, change the signature from `Result<(), Vec<String>>` to `Result<(), Vec<ConfigValidationError>>`.

Replace the body to return structured `ConfigValidationError` entries for each check. Each error includes `field`, `message`, `value`, and `severity: ValidationSeverity::Error`. Import `ConfigValidationError` and `ValidationSeverity` from the error module.

- [ ] **Step 2: Update call site in `main.rs`**

In `src/main.rs`, the validation error printing already uses `Display` via `{err}`, which works with the new type. No change needed to the printing logic.

- [ ] **Step 3: Run check**

Run: `cargo check`
Expected: zero warnings, zero errors

- [ ] **Step 4: Commit**

```bash
git add src/config/mod.rs
git commit -m "feat: upgrade AppConfig::validate() to return structured ConfigValidationError"
```

---

### Task 3: Wire ProviderRegistry as ConfigSubscriber

**Files:**
- Modify: `src/providers/mod.rs`
- Modify: `src/providers/registry.rs`

**Interfaces:**
- Consumes: `ConfigSubscriber`, `ConfigSnapshot` from `config::manager`; `ReloadError` from `config::error`
- Produces: `ProviderRegistry` implements `ConfigSubscriber` with `prepare()`/`commit()`

- [ ] **Step 1: Make `CircuitBreakingProvider` public**

In `src/providers/mod.rs`, add:
```rust
pub mod circuit_breaking_provider;
pub use circuit_breaking_provider::CircuitBreakingProvider;
```
Remove the `#[allow(dead_code)]` from the file-level attribute if `CircuitBreakingProvider` is the only dead code.

- [ ] **Step 2: Add `candidates` field to ProviderRegistry**

In `src/providers/registry.rs`, add to the struct:
```rust
candidates: parking_lot::RwLock<Option<HashMap<String, Arc<ProviderTarget>>>>,
```
Initialize in constructor: `candidates: parking_lot::RwLock::new(None),`

- [ ] **Step 3: Implement ConfigSubscriber for ProviderRegistry**

Add the impl block in `src/providers/registry.rs`:
- `prepare()`: read `new.config.providers`, build complete `ProviderTarget` instances with `CircuitBreaker` and factory closures. Store in `candidates` RwLock.
- `commit()`: take from `candidates`, compute diff (added/removed/updated), log structured event, atomically replace `targets`.

- [ ] **Step 4: Run check**

Run: `cargo check`
Expected: zero warnings, zero errors

- [ ] **Step 5: Commit**

```bash
git add src/providers/mod.rs src/providers/registry.rs
git commit -m "feat: wire ProviderRegistry as ConfigSubscriber with two-phase provider lifecycle"
```

---

### Task 4: Add config_generation to ExecutionContext

**Files:**
- Modify: `src/types/execution_context.rs`

**Interfaces:**
- Consumes: none
- Produces: `ExecutionContext` with `config_generation: u64`

- [ ] **Step 1: Add field to struct**

In `src/types/execution_context.rs`, add `pub config_generation: u64` to the `ExecutionContext` struct.

- [ ] **Step 2: Update construction sites**

Search for `ExecutionContext {` across the codebase. Add `config_generation: 0` at each construction site.

- [ ] **Step 3: Run check**

Run: `cargo check`
Expected: zero warnings, zero errors

- [ ] **Step 4: Commit**

```bash
git add src/types/execution_context.rs
git commit -m "feat: add config_generation field to ExecutionContext"
```

---

### Task 5: Wire ConfigManager into AppState and main.rs

**Files:**
- Modify: `src/server/handlers.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `ConfigManager`, `ConfigSnapshot` from `config::manager`; `ProviderRegistry` with `ConfigSubscriber` impl
- Produces: `AppState` with `config_manager: Arc<ConfigManager>` instead of `config: Arc<AppConfig>`; SIGHUP handler in main

- [ ] **Step 1: Update AppState struct**

In `src/server/handlers.rs`:
- Add `use crate::config::manager::ConfigManager;`
- Replace `pub config: Arc<AppConfig>` with `pub config_manager: Arc<ConfigManager>`

- [ ] **Step 2: Update AppState::new() signature and body**

Change the constructor to accept `config_path: PathBuf` as the last parameter. Build `ConfigManager` inside the constructor with `ProviderRegistry` as a subscriber.

- [ ] **Step 3: Update chat_completions handler**

Change `state.config.to_policies()` to:
```rust
let snapshot = state.config_manager.snapshot();
let policies = snapshot.config.to_policies();
```

- [ ] **Step 4: Restructure main.rs**

Replace hardcoded `ProviderRouter`/`ProviderTarget` construction with `ProviderRegistry`. Create `ConfigManager` via `AppState::new()`. Add `reload_signal` as a `tokio::spawn` background task. Add the `reload_signal` async function using `tokio::signal::unix::SignalKind::hangup()`.

- [ ] **Step 5: Run check**

Run: `cargo check`
Expected: zero warnings, zero errors

- [ ] **Step 6: Commit**

```bash
git add src/server/handlers.rs src/main.rs
git commit -m "feat: wire ConfigManager and ProviderRegistry into AppState, add SIGHUP reload task"
```

---

### Task 6: Update config/default.yaml with correct provider examples

**Files:**
- Modify: `config/default.yaml`

- [ ] **Step 1: Update the providers section**

Edit `config/default.yaml`, replace the providers section with authoritative examples. Include comments explaining `api_key_env`, `failure_threshold`, and `cooldown_secs`. Ensure every example is valid against `AppConfig::validate()`.

- [ ] **Step 2: Commit**

```bash
git add config/default.yaml
git commit -m "docs: update config/default.yaml provider section to authoritative state"
```

---

### Task 7: Unit tests for ConfigManager transactional reload

**Files:**
- Create: `tests/unit/config_manager.rs`
- Test: `cargo test --test unit_tests`

- [ ] **Step 1: Create the test file**

Create `tests/unit/config_manager.rs` with a `MockSubscriber` implementing `ConfigSubscriber` and these tests:

1. `test_reload_succeeds` - assert generation increments, prepare+commit called
2. `test_reload_rolls_back_on_subscriber_rejection` - assert generation unchanged, commit NOT called
3. `test_generation_increments_on_each_reload` - assert gen 1->2->3 across 2 reloads
4. `test_snapshot_immutability` - assert snapshot taken before reload is unchanged after reload
5. `test_subscriber_priority_ordering` - two subscribers with different priorities, assert prepare called in priority order
6. `test_idempotent_reload` - reload identical config, assert no generation increment

- [ ] **Step 2: Run tests**

Run: `cargo test --test unit_tests -- config_manager`
Expected: all pass

- [ ] **Step 3: Commit**

```bash
git add tests/unit/config_manager.rs
git commit -m "test: add ConfigManager transactional reload unit tests"
```

---

### Task 8: Integration tests for SIGHUP and live provider updates

**Files:**
- Create or modify: test files as appropriate
- Test: `cargo test --test integration_tests`

- [ ] **Step 1: Add integration tests for ConfigManager + ProviderRegistry**

Add tests covering:
1. Provider live update via config change and reload
2. Provider removal via config change and reload
3. Invalid config on reload keeps old config active
4. Provider diff verification (added/removed/updated logs)

- [ ] **Step 2: Run tests**

Run: `cargo test`
Expected: all tests pass, including all existing tests

- [ ] **Step 3: Final full check**

Run: `cargo check`
Expected: zero warnings, zero errors

- [ ] **Step 4: Commit**

```bash
git add tests/
git commit -m "test: add integration tests for SIGHUP reload and live provider updates"
```
