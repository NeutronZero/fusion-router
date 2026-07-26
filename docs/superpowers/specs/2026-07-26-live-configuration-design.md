# Live Configuration — Sprint 1.1 Design

> **Epic:** G — Live Configuration (v0.11 Stage 1)
> **Status:** Design approved
> **Date:** 2026-07-26

---

## 1. Architecture

### Core components

```
┌──────────────────────────────────────────────────────────────┐
│                      ConfigManager                           │
│                                                              │
│  ┌────────────────┐  ┌──────────────┐  ┌─────────────────┐   │
│  │ ArcSwap         │  │ config_path  │  │ Vec<Box<dyn     │   │
│  │ <ConfigSnapshot>│  │ (PathBuf)    │  │  ConfigSubscri- │   │
│  └────────────────┘  └──────────────┘  │  ber>>           │   │
│                                         └─────────────────┘   │
│                                                               │
│  reload() → parse → validate → prepare → commit or rollback   │
└──────────────────────────────────────────────────────────────┘
         │
         │ snapshot()
         ▼
   ┌──────────────────┐
   │  ConfigSnapshot   │
   │  ┌──────────────┐ │
   │  │ generation   │ │
   │  │ u64          │ │
   │  ├──────────────┤ │
   │  │ config       │ │
   │  │ Arc<AppConf> │ │
   │  └──────────────┘ │
   └──────────────────┘
         │
         ├── Request-scoped: snapshot() at start, use for entire request
         │
         └── Subscriber: prepare(old, new) → commit(gen) after swap
```

### ConfigSnapshot

```rust
#[derive(Clone)]
pub struct ConfigSnapshot {
    pub generation: u64,
    pub config: Arc<AppConfig>,
}
```

The `ArcSwap` stores a `ConfigSnapshot`, not a bare `AppConfig`. This makes generation and config atomically consistent — it is impossible to read generation N+1 with config N.

### ConfigManager

```rust
pub struct ConfigManager {
    inner: ArcSwap<ConfigSnapshot>,
    config_path: PathBuf,
    subscribers: Vec<Box<dyn ConfigSubscriber + Send + Sync>>,
    generation: AtomicU64,
}
```

- `config_path` is resolved once at startup (from `FUSION_CONFIG` env var or default). Not re-read on each reload.
- `inner` stores the current live snapshot.
- `subscribers` is populated at construction and is immutable post-startup.
- `generation` is atomically incremented on each successful reload.

**Methods:**
- `snapshot() -> ConfigSnapshot` — returns the current live snapshot (clone of Arc, cheap)
- `async fn reload() -> Result<u64, ReloadError>` — the transactional reload entry point

### ConfigSubscriber trait

```rust
pub enum ValidationSeverity {
    Error,
    Warning,
}

pub struct ConfigValidationError {
    pub field: String,
    pub message: String,
    pub value: Option<String>,
    pub severity: ValidationSeverity,
}

pub struct PrepareResult {
    pub warnings: Vec<ConfigValidationError>,
}

impl From<()> for PrepareResult {
    fn from(_: ()) -> Self { Self { warnings: vec![] } }
}

pub trait ConfigSubscriber: Send + Sync {
    fn priority(&self) -> u8 { 0 }

    fn prepare(
        &self,
        old: &ConfigSnapshot,
        new: &ConfigSnapshot,
    ) -> Result<PrepareResult, ReloadError>;

    fn commit(&self, generation: u64);
}
```

**Two-phase protocol:**
1. `prepare()` — called for each subscriber in priority order. Receives old and new snapshots. Returns `PrepareResult` (warnings) or `ReloadError`. This is where subscribers build candidate state (e.g., construct `ProviderTarget` instances) without mutating live state.
2. `commit()` — called only if every subscriber's `prepare()` succeeded. Subscribers perform the atomic swap from candidate to live state.

This prevents partial state updates: if subscriber B rejects after subscriber A prepared, subscriber A's `commit()` is never called.

---

## 2. Provider Wiring

### Current → Target

| Aspect | Current (v0.10) | Target (Sprint 1.1) |
|--------|-----------------|---------------------|
| Provider routing | `ProviderRouter` (static chain in main.rs) | `ProviderRegistry` (dynamic, subscribes to ConfigManager) |
| Provider config | `providers:` YAML section parsed but unused | `providers:` YAML drives ProviderRegistry |
| Provider configuration | Hardcoded factory closures | `ProviderConfig` → `ProviderTarget` construction |
| Circuit breaker | `CircuitBreakingProvider` exists but dead code | Activated as standard wrapper |
| API keys | `dotenv` + env vars, read at startup | Env vars referenced by name from YAML, resolved at reload |

### ProviderRegistry as ConfigSubscriber

```rust
impl ConfigSubscriber for ProviderRegistry {
    fn priority(&self) -> u8 { 10 }

    fn prepare(&self, old: &ConfigSnapshot, new: &ConfigSnapshot) -> Result<PrepareResult, ReloadError> {
        let mut candidates = HashMap::new();

        for (name, cfg) in &new.config.providers {
            let api_key = std::env::var(&cfg.api_key_env)
                .map_err(|_| format!("Missing {} for provider '{}'", cfg.api_key_env, name))?;

            let target = ProviderTarget::new(
                name.clone(),
                CircuitBreaker::new(cfg.failure_threshold, 3, cfg.cooldown_secs),
                Box::new(move || -> Arc<dyn ChatProvider + Send + Sync> {
                    // factory based on provider type
                }),
            );

            candidates.insert(name.clone(), Arc::new(CircuitBreakingProvider::new(target)));
        }

        self.candidates.store(Some(candidates));
        Ok(PrepareResult::default())
    }

    fn commit(&self, generation: u64) {
        if let Some(candidates) = self.candidates.swap(None) {
            let diff = self.diff(&candidates);
            tracing::info!(generation, added = ?diff.added, removed = ?diff.removed, updated = ?diff.updated, "ProviderRegistry commit");
            self.targets = candidates;
        }
    }
}
```

`prepare()` builds a **complete candidate provider set** (all construction, env lookup, and wrapping happens here). `commit()` atomically swaps the prepared set into place and logs a structured diff (added/removed/updated providers with generation).

---

## 3. Config Validation

### Reload flow

```
parse YAML from config_path
        │
        ▼
AppConfig::validate()
   │                              \
   │  Pass                         │  Fail
   ▼                              │
notify subscribers: prepare()     │  log structured error
   │                              │  return Err(rollback)
   │  All OK?                     │
   │  ├─ YES → commit()           │
   │  └─ NO  → rollback           │
   │         (no commit called)   │
   ▼                              │
increment generation              │
store new ConfigSnapshot          │
```

### Validation error type

```rust
pub struct ConfigValidationError {
    pub field: String,
    pub message: String,
    pub value: Option<String>,
    pub severity: ValidationSeverity,
}

pub enum ValidationSeverity {
    Error,
    Warning,
}
```

### ReloadError

```rust
pub enum ReloadError {
    Parse(String),
    Validation(Vec<ConfigValidationError>),
    Subscriber { name: String, reason: String },
}
```

Startup behavior unchanged: load → validate → panic on failure.
Reload behavior: load → validate → rollback on failure → log structured event → continue with old config.

### config/default.yaml

Updated with correct provider examples. Provider section now authoritative. No schema changes, file splitting, or reorganizing in Sprint 1.1.

---

## 4. SIGHUP Integration

### Signal handling

```rust
#[cfg(unix)]
async fn reload_signal(config_manager: Arc<ConfigManager>) {
    let mut stream = tokio::signal::unix::signal(
        tokio::signal::unix::SignalKind::hangup(),
    ).expect("failed to install SIGHUP handler");

    while stream.recv().await.is_some() {
        // config_path is owned by ConfigManager, resolved at startup
        match config_manager.reload().await {
            Ok(gen) => tracing::info!(generation = gen, "configuration reloaded"),
            Err(e) => tracing::error!(error = %e, "reload failed, continuing with previous config"),
        }
    }
}
```

### Integration in main()

```rust
let cm = Arc::new(ConfigManager::new(config_path, subscribers));
let cm_for_reload = Arc::clone(&cm);

tokio::spawn(async move {
    reload_signal(cm_for_reload).await;
});

// existing server startup + shutdown_signal().await
```

`reload_signal()` runs as a background task, not in the main `tokio::select!`. This separates the reload lifecycle from shutdown.

### Windows

Deferred. A periodic file-watch or admin endpoint will be added when Windows support is scoped.

### Structured lifecycle events

```rust
#[derive(Debug)]
pub enum ConfigEvent {
    ReloadStarted { from_gen: u64, to_gen: u64 },
    ReloadSucceeded { generation: u64 },
    ReloadFailed { from_gen: u64, reason: String },
}
```

Emitted via tracing. Integrates with Operational Intelligence (Epic E) in later stages.

---

## 5. Generation in ExecutionContext

Every request carries the config generation it started with:

```rust
pub struct ExecutionContext {
    // ... existing fields ...
    pub config_generation: u64,
}
```

Set at request start via `context.config_generation = config_manager.snapshot().generation`.

This preserves deterministic request semantics: even if SIGHUP arrives mid-request, the in-flight request continues with its original generation. All spans, metrics, replay snapshots, and audit records include this generation.

---

## 6. Testing Strategy

### Unit tests

| Test | Invariant |
|------|-----------|
| `reload_succeeds` | Valid config → generation increments, snapshot updated |
| `reload_rolls_back_on_parse_error` | Malformed YAML → generation unchanged, old config active |
| `reload_rolls_back_on_validation_error` | Invalid field → generation unchanged, error returned |
| `reload_rolls_back_on_subscriber_rejection` | Any `prepare()` returns Err → no `commit()`, no generation increment |
| `snapshot_immutability` | Clone snapshot, modify original → snapshot unchanged |
| `generation_increments` | Each successful reload → gen += 1 |
| `subscriber_priority_ordering` | Subscribers called in priority order during `prepare()` |
| `atomicity_one_subscriber_fails` | One subscriber fails → no subscriber's `commit()` called |
| `prepare_called_commit_not_called` | Rejected subscriber → `commit()` never invoked for any subscriber |
| `idempotent_reload` | Identical config → no generation increment, no subscriber mutation |
| `provider_diff` | Changed providers → structured added/removed/updated logged |
| `provider_diff_no_change` | Identical providers → empty diff |
| `provider_build_from_config` | Valid ProviderConfig → correct ProviderTarget |

### Integration tests

| Test | Scenario |
|------|----------|
| `sighup_triggers_reload` | Send SIGHUP → generation increments |
| `provider_live_update` | Change provider config in file, SIGHUP → new provider routable |
| `provider_removal` | Remove provider from YAML, SIGHUP → provider no longer routable |
| `invalid_config_keeps_old` | Corrupt YAML → SIGHUP → reload fails → old config active |
| `concurrent_requests_during_reload` | In-flight requests use original generation, unaffected by reload |
| `generation_consistency_through_request` | request.snapshot(g=12) → planning → execution → telemetry → all report g=12 |

### Existing tests that must remain green

All existing unit tests, phase invariants, golden tests, deterministic lowering, and regression tests.

---

## 7. What's Not in Sprint 1.1

- Windows SIGHUP equivalent
- File watcher (`notify` crate)
- Cross-version config migration
- Load testing under rapid reload
- Config splitting into `resources/` or `policies/` fragments
- New config schema or fields
- Renaming existing config fields
