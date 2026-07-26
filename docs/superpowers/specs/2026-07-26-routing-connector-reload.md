# Sprint 1.3 — Live Routing Policies & Connector Reload

> **Theme:** Completing Epic G — live routing policy updates and hot-swappable connectors via ConfigSubscriber.
> **Status:** Draft Design
> **Dependencies:** Sprint 1.1 (ConfigManager, ConfigSubscriber, ProviderRegistry subscriber), Sprint 1.2 (MeteredStream)

---

## 1. Problem Statement

### Live Routing Policies
The `ProviderRegistry::prepare()` already rebuilds provider targets with updated circuit breaker thresholds from config. The remaining gaps:

| Gap | Detail |
|-----|--------|
| No `set_thresholds()` on CircuitBreaker | Per-breaker updates without full rebuild |
| No routing rule config | No `routing:` section in AppConfig for prefix mappings, weight, priority |
| No verification | Tests don't verify config change → threshold change → behavior change |

### Connector Reload
The `ConnectorResolver` exists but is **not wired into production**. Connectors are test-only:

| Gap | Detail |
|-----|--------|
| No `unregister_connector()` | Can't remove connectors |
| No `ConnectorConfig` | No config schema for connector definitions |
| No `ConfigSubscriber` | No reload support |
| Not in `AppState` | Not accessible at runtime |
| No factory | No way to create connectors from config |

---

## 2. Architecture

### 2.1 Routing Policy Updates

The `ProviderRegistry` subscriber from Sprint 1.1 already handles the core use case: config change → reload → new `ProviderTarget` instances with updated thresholds. The additions:

1. **CircuitBreaker dynamic thresholds** — Add `update_thresholds()` to `CircuitBreaker` using atomic fields so thresholds can be updated without replacing the entire `ProviderTarget`
2. **Routing config schema** — Optional `routing:` section in `AppConfig` for prefix overrides and default provider selection
3. **RoutingSubscriber** — New `ConfigSubscriber` that reads routing config and updates `ProviderRouter` prefix mappings

But given that `ProviderRegistry::prepare()` already handles provider-level threshold updates, the routing policy feature is mostly **verification and hardening** of the existing infrastructure.

### 2.2 Connector Reload

New subscriber following the `ProviderRegistry` pattern:

```
AppConfig.connectors ──→ ConnectorSubscriber::prepare()
                                │
                    Validate connector types + params
                    Build candidate HashMap<String, Arc<dyn Connector>>
                                │
                    ConnectorSubscriber::commit()
                                │
                    Compute diff (added/removed/updated)
                    Atomic swap in ConnectorResolver
                    Log structured event
```

### 2.3 Connector Factory

```rust
fn create_connector(name: &str, cfg: &ConnectorConfig) -> Result<Arc<dyn Connector>, ReloadError>
```

Maps `connector_type` string to concrete connector implementation with config.

---

## 3. File Map

```
src/scheduler/connector_resolver.rs    # MODIFY: add unregister_connector(), clear()
src/config/mod.rs                      # MODIFY: add ConnectorConfig struct + connectors field
src/config/error.rs                    # MODIFY: add ConnectorError variant to ReloadError
src/providers/circuit_breaker.rs       # MODIFY: add update_thresholds() with atomic fields
src/scheduler/connector_subscriber.rs  # NEW: ConnectorSubscriber implementing ConfigSubscriber
src/server/handlers.rs                 # MODIFY: add ConnectorResolver to AppState
src/main.rs                            # MODIFY: wire ConnectorResolver, register subscriber
config/default.yaml                    # MODIFY: add connectors section
tests/config_reload_tests.rs           # MODIFY: add connector reload tests
```

---

## 4. Testing Strategy

### Unit tests
- `CircuitBreaker::update_thresholds()` — verify threshold change affects behavior
- `ConnectorSubscriber::prepare()` — valid config → Ok, invalid config → Err with ConnectorError
- `ConnectorSubscriber::commit()` — verify connector map updated
- `ConnectorResolver::unregister_connector()` — verify removal

### Integration tests
- `test_routing_policy_reload` — change failure_threshold, reload, verify circuit breaker behavior
- `test_connector_reload_add` — add connector via config, reload, verify registered
- `test_connector_reload_remove` — remove connector via config, reload, verify unregistered
- `test_connector_reload_invalid_type` — bad connector_type in config → reload rejected
