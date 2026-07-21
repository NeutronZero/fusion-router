# Production Hardening & Reliability — Implementation Plan (v0.7.0)

**Goal:** Transform FusionRouter into a production-ready service with security (API key auth, CORS), reliability (rate limiting, health checks, graceful shutdown), and observability (structured JSON logging, request ID tracing).

**Architecture:** All new features implemented as Axum middleware layers or standalone modules registered in `src/server/handlers.rs`. Startup pipeline gains a validation step before the server binds.

**Tech Stack:** Rust, Axum, Tower, DashMap, UUID, Tracing, Tokio, Serde.

**Global Constraints:**
- All new features opt-in via config (disabled by default) — preserve backward compatibility.
- Every task includes a failing test step before implementation.
- All tests must pass after each task (`cargo test`).
- Commit after each task with a descriptive message.

---

## Task 1: Extend Configuration Structs

**Files:** `src/config.rs`, `config/default.yaml`

**Sub-steps:**
- [ ] 1.1 Write failing test expecting new config fields
- [ ] 1.2 Implement `AuthConfig`, `RateLimitingConfig`, `LoggingConfig`, `CorsConfig`, `ServerConfig`
- [ ] 1.3 Implement `Default` for new structs
- [ ] 1.4 Update `config/default.yaml` with defaults
- [ ] 1.5 `cargo test config::tests --lib -v`
- [ ] 1.6 Commit

---

## Task 2: API Key Authentication Middleware

**Files:** `src/middleware/auth.rs`, `src/middleware/mod.rs`, `src/server/handlers.rs`

**Sub-steps:**
- [ ] 2.1 Write failing test (401 without key when enabled)
- [ ] 2.2 Implement `auth_middleware` (check x-api-key, whitelist /health /ready /metrics)
- [ ] 2.3 Register in handlers.rs
- [ ] 2.4 Pass `AuthConfig` via Extension/State
- [ ] 2.5 `cargo test middleware::auth --lib -v`
- [ ] 2.6 Commit

---

## Task 3: CORS Middleware

**Files:** `src/middleware/cors.rs`, `src/server/handlers.rs`

**Sub-steps:**
- [ ] 3.1 Write failing integration test for CORS headers
- [ ] 3.2 Implement `cors_layer_from_config()` using `tower_http::cors::CorsLayer`
- [ ] 3.3 Wire into router
- [ ] 3.4 `cargo test integration::cors --test integration_tests -v`
- [ ] 3.5 Commit

---

## Task 4: Rate Limiting Middleware

**Files:** `src/middleware/rate_limit.rs`, `src/server/handlers.rs`

**Sub-steps:**
- [ ] 4.1 Write failing unit test for token bucket
- [ ] 4.2 Implement `DashMap<ClientId, Bucket>` token-bucket logic
- [ ] 4.3 Background cleanup task (prune idle buckets)
- [ ] 4.4 Middleware that extracts client identity and checks rate
- [ ] 4.5 Wire into handlers after auth
- [ ] 4.6 Integration test (rapid requests → 429)
- [ ] 4.7 `cargo test middleware::rate_limit --lib -v && cargo test integration::rate_limit --test integration_tests -v`
- [ ] 4.8 Commit

---

## Task 5: Health Checks Endpoints

**Files:** `src/server/health.rs`, `src/server/handlers.rs`

**Sub-steps:**
- [ ] 5.1 Write failing unit test
- [ ] 5.2 Implement `/health` → `{"status":"ok"}`
- [ ] 5.3 Implement `/ready` → checks SQLite, plugins, provider pool
- [ ] 5.4 Wire routes without auth
- [ ] 5.5 `cargo test server::health --lib -v`
- [ ] 5.6 Commit

---

## Task 6: Graceful Shutdown

**Files:** `src/server/shutdown.rs`, `src/main.rs`

**Sub-steps:**
- [ ] 6.1 Write failing integration test (SIGTERM → drain)
- [ ] 6.2 Implement `shutdown_signal()` (SIGTERM/SIGINT + timeout)
- [ ] 6.3 Update `main.rs` → `with_graceful_shutdown(shutdown)`
- [ ] 6.4 Add logging at start/end of shutdown
- [ ] 6.5 `cargo test server::shutdown --lib -v`
- [ ] 6.6 Commit

---

## Task 7: Structured JSON Logging

**Files:** `src/telemetry/logging.rs`, `src/main.rs`, `Cargo.toml`

**Sub-steps:**
- [ ] 7.1 Write failing unit test for subscriber config
- [ ] 7.2 Implement `setup_logging()` (text/json switch, file output via tracing-appender)
- [ ] 7.3 Call at start of `main`
- [ ] 7.4 Add `tracing-appender` dep
- [ ] 7.5 `cargo test telemetry::logging --lib -v`
- [ ] 7.6 Commit

---

## Task 8: Request ID Tracing Middleware

**Files:** `src/middleware/request_id.rs`, `src/server/handlers.rs`

**Sub-steps:**
- [ ] 8.1 Write failing test (generated/preserved x-request-id)
- [ ] 8.2 Implement middleware (read or generate UUID, inject into tracing span + response header)
- [ ] 8.3 Wire as outermost layer
- [ ] 8.4 Update handlers to use `Extension<RequestId>`
- [ ] 8.5 `cargo test middleware::request_id --lib -v`
- [ ] 8.6 Commit

---

## Task 9: Configuration Validation

**Files:** `src/validation.rs`, `src/main.rs`

**Sub-steps:**
- [ ] 9.1 Write failing tests for each invalid config case
- [ ] 9.2 Implement `validate_config()` with all checks from design spec
- [ ] 9.3 Call in `main` after config load, exit on failure
- [ ] 9.4 Golden tests with malformed YAML
- [ ] 9.5 `cargo test validation --lib -v && cargo test golden::config_validation --test golden_tests -v`
- [ ] 9.6 Commit

---

## Task 10: Integrate All Middleware into Server

**Files:** `src/server/handlers.rs`, `src/main.rs`

**Sub-steps:**
- [ ] 10.1 Order: Request ID → CORS → Auth → Rate Limiting
- [ ] 10.2 Pass AppState to middleware
- [ ] 10.3 Exempt /health, /ready, /metrics from auth and rate limiting
- [ ] 10.4 Integration tests for full middleware stack
- [ ] 10.5 `cargo test`
- [ ] 10.6 Commit

---

## Task 11: Update Configuration File

**Files:** `config/default.yaml`

**Sub-steps:**
- [ ] 11.1 Add auth, rate_limiting, logging, server.cors sections
- [ ] 11.2 `cargo run -- --config config/default.yaml` to verify
- [ ] 11.3 Commit

---

## Task 12: Comprehensive Testing

**Files:** `tests/integration/`, `tests/golden/`

**Sub-steps:**
- [ ] 12.1 Integration tests for all features
- [ ] 12.2 Golden tests for config validation
- [ ] 12.3 Verify all existing tests pass
- [ ] 12.4 `cargo test`
- [ ] 12.5 Commit

---

## Task 13: Version Bump and Release v0.7.0

**Files:** `Cargo.toml`, `CHANGELOG.md`

**Sub-steps:**
- [ ] 13.1 Bump version to 0.7.0
- [ ] 13.2 Update CHANGELOG.md
- [ ] 13.3 `cargo build --release && cargo test`
- [ ] 13.4 Commit, tag v0.7.0, push
- [ ] 13.5 Create GitHub release
