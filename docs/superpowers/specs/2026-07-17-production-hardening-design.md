# Production Hardening & Reliability — Design Spec (v0.7.0)

## Objective

Evolve FusionRouter from a feature-rich prototype into a production-grade service:
secure (auth, CORS), reliable (rate limiting, health checks, graceful shutdown),
and observable (JSON logging, request ID tracing).

## Must-Haves (Core Scope)

| Feature | Effort | Dependencies |
|---------|--------|-------------|
| API Key Authentication | 1 day | axum middleware |
| Rate Limiting | 1 day | dashmap |
| Health Checks (/health, /ready) | 0.5 day | axum routes |
| Graceful Shutdown | 0.5 day | tokio signal |
| Structured JSON Logging | 0.5 day | tracing-subscriber, tracing-appender |
| Configuration Validation | 1 day | anyhow |
| Request ID Tracing | 0.5 day | uuid, axum middleware |
| CORS Support | 0.5 day | tower-http (already present) |

**Total:** ~5.5 days

## Architecture Changes

### Overview

All new features are implemented as **Axum middleware layers** or **standalone modules**
registered in `src/server/handlers.rs`. The startup pipeline in `src/main.rs`
gains a validation step before the server binds.

### Module Map

```
src/
  middleware/
    mod.rs          — re-exports
    auth.rs         — API key validation
    cors.rs         — CORS configuration
    rate_limit.rs   — token-bucket rate limiter
    request_id.rs   — x-request-id propagation
  server/
    health.rs       — /health and /ready endpoints
    shutdown.rs     — graceful shutdown future
  config.rs         — new struct fields (no new file)
  validation.rs     — config validation at startup
  telemetry/
    logging.rs      — structured logging setup
```

---

## Section 1: Security

### API Key Authentication

**Config** (`config/default.yaml`):
```yaml
auth:
  enabled: false           # opt-in; false = no auth middleware
  api_keys: []             # list of valid x-api-key values
```

**Middleware** (`src/middleware/auth.rs`):
- Axum `from_fn` middleware
- If `auth.enabled == false`: pass through (no-op)
- Check `x-api-key` header against `api_keys` list
- Whitelist from auth: `/health`, `/ready`, `/metrics`
- Return 401 `{"error":"unauthorized"}` on mismatch

**Testing:**
- Unit: valid key passes, invalid key returns 401, disabled = pass-through
- Integration: hit `/chat` without key → 401; with valid key → 200

### CORS

**Config** (under `[server]`):
```yaml
server:
  cors:
    allowed_origins: ["*"]
    allowed_methods: ["GET", "POST", "PUT", "DELETE", "OPTIONS"]
    allowed_headers: ["content-type", "authorization", "x-api-key", "x-request-id"]
```

**Implementation** (`src/middleware/cors.rs`):
- Wire `tower_http::cors::CorsLayer` from config values
- Default `allowed_origins: ["*"]` for dev; empty = deny all
- Already have `tower-http` with `cors` feature in Cargo.toml

---

## Section 2: Reliability

### Rate Limiting

**Config** (`config/default.yaml`):
```yaml
rate_limiting:
  requests_per_minute: 60
  burst_size: 10
  cleanup_interval_secs: 300   # prune idle buckets every 5 min
```

**Implementation** (`src/middleware/rate_limit.rs`):
- Token-bucket algorithm per client identity
- Client identity: `x-api-key` > `x-forwarded-for` > `remote_addr`
- State stored in `DashMap<ClientId, Bucket>` (already have `dashmap` dep)
- Background tokio task prunes buckets with `last_access > cleanup_interval`
- Response: 429 `{"error":"rate_limit_exceeded","retry_after_secs": N}`

**Testing:**
- Unit: burst consumed then requests blocked, cleanup prunes old entries
- Integration: rapid requests → 429 after burst

### Graceful Shutdown

**Config** (under `[server]`):
```yaml
server:
  shutdown_timeout_secs: 30
```

**Implementation** (`src/server/shutdown.rs`):
- Install signal handlers: `ctrl_c()` (cross-platform) + `SIGTERM` (unix) / `SetConsoleCtrlHandler` (windows)
- Pass future to `axum::serve(...).with_graceful_shutdown(shutdown_signal)`
- Log start and completion of shutdown
- Timeout: if in-flight requests exceed `shutdown_timeout_secs`, force exit

**Testing:**
- Unit: signal future resolves correctly, timeout future fires
- Integration: send SIGTERM, verify 200 responses during drain

### Health Checks

**Implementation** (`src/server/health.rs`):

`GET /health` — Liveness
```json
{"status": "ok"}
```

`GET /ready` — Readiness
```json
{
  "status": "ok",
  "checks": {
    "database": "ok",
    "plugins": "ok",
    "providers": "ok"
  }
}
```
- Database check: `PRAGMA quick_check` on SQLite
- Plugins check: all loaded manifests respond
- Providers check: at least one provider in pool is reachable
- Any check fails → `{"status": "not_ready", "checks": {...}}`, HTTP 503

**Testing:**
- Unit: `/health` always succeeds, `/ready` reflects state
- Integration: startup → `/ready` eventually returns ok

---

## Section 3: Observability

### Structured JSON Logging

**Config** (`config/default.yaml`):
```yaml
logging:
  format: "text"          # "text" | "json"
  level: "info"           # "debug" | "info" | "warn" | "error"
  directory: ""           # empty = stderr; non-empty = file output
```

**Implementation** (`src/telemetry/logging.rs`):
- On startup, configure `tracing_subscriber::fmt()` with `json()` when `format = "json"`
- When `directory` is set, use `tracing_appender::non_blocking(RollingFileAppender::new(
    Rotation::DAILY, directory, "fusion-router.log"))` for non-blocking file writes
- Respect `level` via `EnvFilter`
- Existing `tracing-subscriber` dep already has `json` feature

**Testing:**
- Unit: verify subscriber configures without panic for each format
- Integration: check log output contains JSON fields when json mode

### Request ID Tracing

**Implementation** (`src/middleware/request_id.rs`):
- Axum middleware that reads `x-request-id` header or generates `Uuid::new_v4()`
- Stores `RequestId` in `request.extensions()`
- Creates `tracing::info_span!("request", request_id = %req_id)` and enters it for request lifetime
- Sets `x-request-id` response header
- `RequestId` accessible via `axum::Extension<RequestId>` in handlers

**Testing:**
- Unit: request-id generated when absent, propagated when present
- Integration: response headers include `x-request-id`

---

## Section 4: Configuration Validation

**Implementation** (`src/validation.rs`):
- Called after `AppConfig::load()` in `main.rs`, before binding the server
- Returns `Result<(), Vec<ConfigError>>` where `ConfigError { field, message }`
- Checks:

| Field | Validation |
|-------|-----------|
| `server.port` | 1024–65535 |
| `server.shutdown_timeout_secs` | > 0 |
| `auth.enabled && auth.api_keys` | `api_keys` must be non-empty |
| `rate_limiting.requests_per_minute` | > 0 |
| `rate_limiting.burst_size` | > 0 |
| `logging.level` | one of "debug", "info", "warn", "error" |
| `logging.directory` | must exist and be writable if non-empty |
| `tools.allowed_shell_commands` | each entry non-empty |
| `tools.allowed_read_directories` | paths must be canonicalizable |
| `providers[*]` | each provider has required fields |

- All errors printed to stderr, process exits with code 1

**Testing:**
- Unit: test each invalid config variant produces the expected error
- Golden: exercise full config with known-bad values

---

## Testing Strategy

| Layer | Tests | Coverage |
|-------|-------|----------|
| Unit | per-module (auth, rate_limit, health, validation, logging) | Individual components |
| Integration | axum test server with middleware stack | End-to-end request flow |
| Golden | config validation with known-bad YAML | Error messages and exit codes |

- All new features are opt-in via config (disabled by default)
- Existing tests continue to pass without new config fields set
- Add `tracing-appender` to Cargo.toml dependencies (for file logging)

## Non-Goals

- No persistence for rate limit state (in-memory only; lost on restart)
- No distributed rate limiting (single-instance only)
- No mTLS or certificate management
- No configuration reload (deferred to v0.8.0 stretch)
