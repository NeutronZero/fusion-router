# Sprint 1.3 — Live Routing Policies & Connector Reload Implementation Plan

**Goal:** Complete Epic G — live routing policy updates via existing ProviderRegistry infrastructure + new ConnectorSubscriber.

---

### Task 1: Add `update_thresholds()` to CircuitBreaker

**Files:**
- Modify: `src/providers/circuit_breaker.rs`

**What:** Change `failure_threshold` from `u32` to `AtomicU32` and `cooldown_duration` to `AtomicU64` (store seconds). Add `update_thresholds(failure_threshold, cooldown_secs)` method.

**Don't change the constructor signature** — keep backward compat.

**Validate:** `cargo check`, `cargo test`

**Commit:** `feat: add update_thresholds() to CircuitBreaker with atomic fields`

---

### Task 2: Add `ConnectorConfig` and `connectors` field to `AppConfig`

**Files:**
- Modify: `src/config/mod.rs`
- Modify: `src/config/error.rs`

**What:**
1. Add `ConnectorConfig` struct with `connector_type: String`, `config: HashMap<String, Value>`
2. Add `pub connectors: HashMap<String, ConnectorConfig>` to `AppConfig` with `#[serde(default)]`
3. Add `ConnectorConfig` (or derive) with defaults
4. Add `ConnectorError(String)` variant to `ReloadError`

**Validate:** `cargo check`, `cargo test`

**Commit:** `feat: add ConnectorConfig and connectors field to AppConfig`

---

### Task 3: Add `unregister_connector()` to `ConnectorResolver`

**Files:**
- Modify: `src/scheduler/connector_resolver.rs`

**What:**
1. Add `pub fn unregister_connector(&self, name: &str) -> bool` — removes from connectors map and capability_map, returns true if existed
2. Add `pub fn clear(&self)` — removes all connectors

**Validate:** `cargo check`, `cargo test`

**Commit:** `feat: add unregister_connector() and clear() to ConnectorResolver`

---

### Task 4: Create `ConnectorSubscriber`

**Files:**
- Create: `src/scheduler/connector_subscriber.rs`

**What:**
1. Struct holds `Arc<RwLock<HashMap<String, Arc<dyn Connector>>>>` pointing to ConnectorResolver's connectors
2. Implements `ConfigSubscriber`
3. `prepare()`: reads `new.config.connectors`, builds candidate connectors via factory, validates each can be created
4. `commit()`: atomically swaps connector map, logs added/removed/updated
5. Priority: 5 (between default 0 and ProviderRegistry's 10)

Connector factory function maps `connector_type` string to concrete connector:

```rust
fn create_connector(
    name: &str,
    cfg: &ConnectorConfig,
) -> Result<Arc<dyn Connector>, ReloadError> {
    match cfg.connector_type.as_str() {
        "http" => Ok(Arc::new(connectors::http::HttpConnector::new(cfg.config.clone()))),
        "shell" => Ok(Arc::new(connectors::shell::ShellConnector::new(cfg.config.clone()))),
        "github" => Ok(Arc::new(connectors::github::GitHubConnector::new(cfg.config.clone()))),
        "filesystem" => Ok(Arc::new(connectors::filesystem::FilesystemConnector::new(cfg.config.clone()))),
        "browser" => Ok(Arc::new(connectors::browser::BrowserConnector::new(cfg.config.clone()))),
        "mcp" => Ok(Arc::new(connectors::mcp::McpConnector::new(cfg.config.clone()))),
        _ => Err(ReloadError::ConnectorError(format!("Unknown connector type: {}", cfg.connector_type))),
    }
}
```

**Validate:** `cargo check`, `cargo test`

**Commit:** `feat: create ConnectorSubscriber for hot-swappable connectors`

---

### Task 5: Wire ConnectorResolver into AppState and main.rs

**Files:**
- Modify: `src/server/handlers.rs`
- Modify: `src/main.rs`

**What:**
1. In `handlers.rs`: Add `pub connector_resolver: Arc<ConnectorResolver>` to AppState. Add `use crate::scheduler::connector_resolver::ConnectorResolver;`
2. In `main.rs`: Create `ConnectorResolver`, register `ConnectorSubscriber` with `config_manager` after `ProviderRegistry` subscriber
3. Register connector subscriber: `state.config_manager.register_subscriber(Box::new(ConnectorSubscriber::new(state.connector_resolver.connectors.clone())));`

**Validate:** `cargo check`, `cargo test`

**Commit:** `feat: wire ConnectorResolver into AppState and register ConnectorSubscriber`

---

### Task 6: Update config/default.yaml

**Files:**
- Modify: `config/default.yaml`

**What:** Add `connectors:` section with example entries

```yaml
connectors:
  my-github:
    connector_type: github
    config:
      token_env: GITHUB_TOKEN
  my-filesystem:
    connector_type: filesystem
    config:
      allowed_paths: ["./data"]
```

**Commit:** `docs: add connectors section to config/default.yaml`

---

### Task 7: Unit tests

Add tests for:
1. CircuitBreaker threshold update changes behavior
2. ConnectorSubscriber validate/invalid config
3. ConnectorResolver unregister

---

### Task 8: Integration tests for connector reload

Add to `tests/config_reload_tests.rs`:
1. Connector add via reload
2. Connector remove via reload  
3. Invalid connector type → reload rejected
