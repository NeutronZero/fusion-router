# Handoff Report: Requirement R1 (Access Control & Authentication Middleware)

## 1. Observation

### 1.1 Source Code Findings in `src/main.rs`
- **Lines 188–199**: Base router initialized with endpoints `/v1/chat/completions`, `/v1/messages`, `/metrics`, `/health`, `/ready`. Global middleware layers are attached immediately afterwards:
  ```rust
  .layer(axum::middleware::from_fn(middleware::auth::auth_middleware))
  .layer(axum::Extension(auth_config))
  ```
- **Lines 229–238**: Operations sub-router (`operations_routes`) created containing `/v1/operations/registry`, `/v1/operations/runtime`, `/v1/operations/metrics`, `/v1/operations/policies` (GET & POST), `/v1/operations/attestations`. Merged into `app` at line 238 via `app = app.merge(operations_routes);`.
- **Lines 242–246**: Execution sub-router (`execution_routes`) created containing `/v1/executions`. Merged into `app` at line 246 via `app = app.merge(execution_routes);`.

### 1.2 Auth Middleware Logic in `src/middleware/auth.rs`
- **Lines 15–25**: Missing `AuthConfig` extension returns HTTP 401 (fail closed).
- **Lines 28–30**: If `auth_config.enabled == false`, request is allowed through without API key.
- **Lines 33–36**: Explicit path whitelist for `/health`, `/ready`, and `/metrics`.
- **Lines 44–50**: Validates `x-api-key` header against `auth_config.api_keys`. Returns HTTP 401 if missing or invalid.

---

## 2. Logic Chain

1. **Axum Middleware Scoping**: Axum `.layer()` wraps only those routes present in the `Router` at the time `.layer()` is called.
2. **Merge Ordering in `src/main.rs`**: `app.layer(auth_middleware)` and `app.layer(Extension(auth_config))` are invoked at lines 196–197 when `app` only contains the initial base routes (`/v1/chat/completions`, `/v1/messages`, `/metrics`, `/health`, `/ready`).
3. **Standalone Sub-Routers**: `operations_routes` and `execution_routes` are built independently without `.layer(auth_middleware)` or `.layer(Extension(auth_config))`.
4. **Post-Layer Merging**: When `app.merge(operations_routes)` and `app.merge(execution_routes)` are called at lines 238 and 246, Axum merges their route tables into `app`, but does **not** apply `app`'s previously attached middleware layers to these new routes.
5. **Execution & Ops Unauthenticated Access**: Consequently, requests to `/v1/executions` and `/v1/operations/*` bypass `auth_middleware` completely. Even with `auth.enabled = true`, unauthenticated requests reach the handler functions.
6. **Resolution**: By initializing `operations_routes` and `execution_routes` and merging them into the base `Router` *before* chaining `.layer(auth_middleware)` and `.layer(Extension(auth_config))`, all routes (including `/v1/executions` and `/v1/operations/*`) are passed through `auth_middleware`.

---

## 3. Caveats

- **Read-Only Scope**: This report is produced under read-only investigation rules. No code edits have been committed to `src/main.rs`.
- **Built-in Whitelist Safety**: `auth_middleware` already contains explicit whitelisting for `/health`, `/ready`, and `/metrics`. Applying `auth_middleware` across the combined router will not inadvertently block health monitoring checks.
- **Other Bypassed Middleware**: In addition to `auth_middleware`, the current post-layer merge also causes `/v1/executions` and `/v1/operations/*` to bypass `rate_limit_middleware`, `request_id_middleware`, `cors_layer`, and `TraceLayer`. The recommended fix solves all of these simultaneously.

---

## 4. Conclusion

The authentication bypass on `/v1/executions` and `/v1/operations/*` when `auth.enabled = true` is strictly caused by router creation and merge ordering in `src/main.rs`. Reordering router assembly so that all sub-routers are merged into the main `Router` prior to chaining `.layer(...)` will enforce API key authentication for all `/v1/*` routes.

---

## 5. Verification Method

### 5.1 Static Verification
Inspect `src/main.rs` after editing: confirm `.merge(operations_routes)` and `.merge(execution_routes)` occur before `.layer(axum::middleware::from_fn(middleware::auth::auth_middleware))` and `.layer(axum::Extension(auth_config))`.

### 5.2 Build & Test Verification
1. `cargo check --all-targets`
2. `cargo test --all-features`

### 5.3 Security Integration Test
Run a test (e.g. in `tests/security.rs`) configuring `AuthConfig { enabled: true, api_keys: vec!["valid-key".into()] }`:
- Request `POST /v1/executions` without `x-api-key` header -> Expect HTTP 401 Unauthorized.
- Request `GET /v1/operations/registry` without `x-api-key` header -> Expect HTTP 401 Unauthorized.
- Request `POST /v1/executions` with `x-api-key: valid-key` -> Expect request to pass auth.
