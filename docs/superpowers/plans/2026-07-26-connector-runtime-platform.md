### Task 1: Version negotiation on ConnectorResolver

Add semver check in `register_connector()`. Define `MIN_SUPPORTED_RUNTIME_VERSION = "0.10.0"`. If connector version < minimum, return Err.

### Task 2: Capability search on ConnectorResolver

Add `search_by_capability(&self, id: &CapabilityId) -> Vec<Arc<dyn Connector>>` and `search_by_permission()` methods. Iterate connectors, match on descriptor capabilities.

### Task 3: ConnectorHealthChecker

File `src/scheduler/connector_health.rs`:
- `ConnectorHealth` struct with `status: HealthStatus`, `last_check: Instant`, `latency_ms: u64`
- `HealthStatus` enum: `Healthy`, `Degraded`, `Unhealthy`
- `ConnectorHealthChecker` struct with handle to resolver, runs on interval
- `check_connector_health(connector)` — calls `connector.descriptor()`, measures latency

### Task 4: Connector metrics

File `src/telemetry/connector_metrics.rs`:
- `fusionrouter_connector_health_status` — gauge per connector (1=healthy, 0=unhealthy)
- `fusionrouter_connector_check_duration_seconds` — histogram
- `fusionrouter_connector_checks_total` — counter with status label

### Task 5: Wire into main.rs

Spawn health checker task. Pass ConnectorResolver ref.

### Task 6: Tests

Unit tests for version check, capability search, health checker.
