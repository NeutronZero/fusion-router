# Sprint 1.4 — Connector Runtime Platform

> **Theme:** Operational layer around the Connector SDK — health monitoring, version negotiation, capability search.
> **Status:** Draft Design
> **Dependencies:** Sprint 1.3 (ConnectorResolver wired, ConnectorSubscriber)

---

## 1. Scope

Epic D features delivered in Sprint 1.4:

| Feature | Scope |
|---------|-------|
| Connector health monitoring | Periodic health checks via `ConnectorHealthChecker` background task |
| Connector version negotiation | SemVer compatibility check on registration |
| Connector capability search | Query connectors by capability, permission, or schema |
| Connector health metrics | Prometheus histograms per connector (latency, error rate, uptime) |

Deferred: marketplace, discovery, sandbox policies.

---

## 2. Architecture

### 2.1 ConnectorHealthChecker

Background task spawned at startup, runs on interval:

```
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        for (name, connector) in resolver.connectors.read().iter() {
            match check_connector_health(connector).await {
                Ok(status) => update_health(name, status),
                Err(e) => mark_unhealthy(name, e),
            }
        }
    }
})
```

### 2.2 Capability Search

```rust
impl ConnectorResolver {
    pub fn search_by_capability(&self, capability: &CapabilityId) -> Vec<Arc<dyn Connector>>
    pub fn search_by_permission(&self, permission: &str) -> Vec<Arc<dyn Connector>>
}
```

### 2.3 Version Negotiation

On `register_connector()`: compare `connector.version` against `MIN_SUPPORTED_RUNTIME_VERSION`. Reject if incompatible.

---

## 3. File Map

```
src/scheduler/connector_health.rs       # NEW — health checker + health status
src/scheduler/connector_resolver.rs     # MODIFY — +version check, +capability search
src/telemetry/connector_metrics.rs      # NEW — per-connector Prometheus metrics
src/main.rs                             # MODIFY — spawn health checker
```

---

## 4. Task Outline

1. Add version negotiation to ConnectorResolver (semver check on register)
2. Add capability search to ConnectorResolver
3. Create ConnectorHealthChecker (background health polling)
4. Create connector metrics (latency/error/uptime per connector)
5. Wire health checker into main.rs
6. Tests
