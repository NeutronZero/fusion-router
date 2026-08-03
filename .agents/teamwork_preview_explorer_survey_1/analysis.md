# Requirement R1: Access Control & Authentication Middleware Investigation Analysis

## 1. Executive Summary

Requirement R1 addresses a critical security vulnerability in `fusion-router`: **unauthenticated access to execution and operation endpoints** when authentication is explicitly enabled (`auth.enabled = true`). 

Specifically:
- `/v1/executions` (workflow execution endpoint)
- `/v1/operations/*` (administrative and inspection endpoints including `/registry`, `/runtime`, `/metrics`, `/policies`, `/attestations`)

These routes bypass `auth_middleware` completely because they are merged into the main Axum `Router` **after** the authentication middleware and state/config extension layers have been applied.

---

## 2. Current Architecture & Route Definitions

### 2.1 Axum Router Construction in `src/main.rs`

In `src/main.rs` (lines 188–246), the server router is assembled in multiple stages:

```rust
// Stage 1: Base Router with Core Endpoints & Middleware Layers (lines 188-199)
let mut app = Router::new()
    .route("/v1/chat/completions", post(server::handlers::chat_completions))
    .route("/v1/messages", post(server::handlers::anthropic_messages))
    .route("/metrics", get(server::handlers::metrics_handler))
    .route("/health", get(server::health::health_handler))
    .route("/ready", get(server::health::ready_handler))
    .layer(TraceLayer::new_for_http())
    .layer(axum::middleware::from_fn(middleware::request_id::request_id_middleware))
    .layer(axum::middleware::from_fn(middleware::auth::auth_middleware))
    .layer(axum::Extension(auth_config))
    .layer(crate::middleware::cors::cors_layer_from_config(&cors_config))
    .with_state(state.clone());

// Stage 2: Rate Limit Middleware Layer (lines 201-207)
if rate_limiting_enabled {
    let limiter = middleware::rate_limit::RateLimiter::new(rate_limiting_config);
    limiter.start_cleanup();
    app = app
        .layer(axum::middleware::from_fn(middleware::rate_limit::rate_limit_middleware))
        .layer(axum::Extension(limiter));
}

// Stage 3: Operations Router Creation & Merge (lines 229-238)
let operations_routes = axum::Router::new()
    .route("/v1/operations/registry", axum::routing::get(crate::operations::handlers::registry_handler))
    .route("/v1/operations/runtime", axum::routing::get(crate::operations::handlers::runtime_handler))
    .route("/v1/operations/metrics", axum::routing::get(crate::operations::handlers::metrics_handler))
    .route("/v1/operations/policies", axum::routing::get(crate::operations::handlers::policies_list_handler))
    .route("/v1/operations/policies", axum::routing::post(crate::operations::handlers::policies_create_handler))
    .route("/v1/operations/attestations", axum::routing::get(crate::operations::handlers::attestations_handler))
    .with_state(ops_state);

app = app.merge(operations_routes); // MERGED POST-LAYERING!

// Stage 4: Execution Router Creation & Merge (lines 242-246)
let execution_routes = axum::Router::new()
    .route("/v1/executions", axum::routing::post(crate::server::execution::execute_workflow_handler))
    .with_state(exec_plane);

app = app.merge(execution_routes); // MERGED POST-LAYERING!
```

---

## 3. Root Cause Analysis (Why the Bypass Occurs)

### 3.1 Axum Middleware Layering Semantics
In Axum framework:
- `.layer(middleware)` wraps **only the routes currently registered in the `Router` instance at the time `.layer()` is invoked**.
- `.merge(other_router)` combines the route dispatch tables of two `Router` instances.
- Calling `app.merge(other_router)` on an `app` that already has layers attached **does NOT apply `app`'s existing layers to `other_router`'s routes**.

### 3.2 Consequences of Current Ordering
Because `operations_routes` and `execution_routes` were instantiated as standalone `Router` instances and merged into `app` *after* `.layer(auth_middleware)` and `.layer(Extension(auth_config))` were called on `app`:

1. **Authentication Bypass**: Incoming requests to `/v1/executions` or `/v1/operations/*` match the merged routes directly. Axum dispatches them to their handlers without passing through `auth_middleware`.
2. **Missing Extensions**: `Extension(auth_config)` is also not present in the extension map for those sub-routers.
3. **Collateral Middleware Bypasses**: `operations_routes` and `execution_routes` also bypass `request_id_middleware`, `cors_layer`, `TraceLayer`, and `rate_limit_middleware`.

---

## 4. Auth Middleware Behavior (`src/middleware/auth.rs`)

Inspection of `src/middleware/auth.rs` reveals the exact authentication logic:

```rust
pub async fn auth_middleware(
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let Some(auth_config) = req.extensions().get::<AuthConfig>().cloned() else {
        // Fail closed if AuthConfig extension missing
        return Err((StatusCode::UNAUTHORIZED, json!({"error": "unauthorized"}).to_string()));
    };

    if !auth_config.enabled {
        return Ok(next.run(req).await);
    }

    let path = req.uri().path();
    let whitelisted = path == "/health" || path == "/ready" || path == "/metrics";
    if whitelisted {
        return Ok(next.run(req).await);
    }

    let api_key = req
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    match api_key {
        Some(key) if auth_config.api_keys.contains(&key) => Ok(next.run(req).await),
        _ => Err((StatusCode::UNAUTHORIZED, json!({"error": "unauthorized"}).to_string())),
    }
}
```

Key characteristics:
1. **Fail-Closed**: If `AuthConfig` extension is missing on a request that passes through `auth_middleware`, it returns HTTP 401 `unauthorized`.
2. **Global Whitelist**: Built-in whitelist for `/health`, `/ready`, and `/metrics`.
3. **API Key Requirement**: All other routes (specifically `/v1/chat/completions`, `/v1/messages`, and when fixed, `/v1/executions` and `/v1/operations/*`) require an `x-api-key` header matching `auth_config.api_keys`.

Because `auth_middleware` already safely whitelists non-v1 operational routes (`/health`, `/ready`, `/metrics`), wrapping the entire combined application router in `auth_middleware` correctly protects **all `/v1/*` endpoints**.

---

## 5. Recommended Fix Strategy

### 5.1 Router Refactoring in `src/main.rs`

Reorder the router construction in `src/main.rs` so that all sub-routers (`operations_routes`, `execution_routes`) are created and merged **BEFORE** any global middleware layers are applied.

#### Proposed Code Replacement for `src/main.rs`:

```rust
    // 1. Construct operations router
    let ops_registry = Arc::new(parking_lot::RwLock::new(crate::capability::InMemoryCapabilityRegistry::new()));
    let ops_cache = Arc::new(crate::operations::RuntimeModuleCache::new());
    let ops_dashboard = Arc::new(crate::operations::dashboard::DefaultDashboardDataProvider::new(
        ops_registry.clone(),
        ops_cache.clone(),
    ));
    let ops_inspector = Arc::new(crate::operations::runtime_inspector::RuntimeInspector::new(ops_cache.clone()));
    let ops_store = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let ops_audit = Arc::new(crate::telemetry::audit::AuditLog::new(1000));
    let ops_policy_admin = Arc::new(crate::operations::policy_admin::PolicyAdmin::new(ops_store, ops_audit.clone()));
    let ops_verifier = Arc::new(crate::operations::MockPackageVerifier);
    let ops_attestation_viewer = Arc::new(crate::operations::attestation_viewer::AttestationViewer::new(ops_verifier, ops_audit));

    let ops_state = crate::operations::handlers::OperationsState {
        dashboard: ops_dashboard,
        inspector: ops_inspector,
        policy_admin: ops_policy_admin,
        attestation_viewer: ops_attestation_viewer,
    };

    let operations_routes = axum::Router::new()
        .route("/v1/operations/registry", axum::routing::get(crate::operations::handlers::registry_handler))
        .route("/v1/operations/runtime", axum::routing::get(crate::operations::handlers::runtime_handler))
        .route("/v1/operations/metrics", axum::routing::get(crate::operations::handlers::metrics_handler))
        .route("/v1/operations/policies", axum::routing::get(crate::operations::handlers::policies_list_handler))
        .route("/v1/operations/policies", axum::routing::post(crate::operations::handlers::policies_create_handler))
        .route("/v1/operations/attestations", axum::routing::get(crate::operations::handlers::attestations_handler))
        .with_state(ops_state);

    // 2. Construct execution router
    let event_bus = Arc::new(crate::events::BroadcastEventBus::new(1024));
    let exec_plane = crate::server::execution::build_execution_plane(event_bus, state.executor.clone());
    let execution_routes = axum::Router::new()
        .route("/v1/executions", axum::routing::post(crate::server::execution::execute_workflow_handler))
        .with_state(exec_plane);

    // 3. Build unified base router and merge all route groups
    let app = Router::new()
        .route("/v1/chat/completions", post(server::handlers::chat_completions))
        .route("/v1/messages", post(server::handlers::anthropic_messages))
        .route("/metrics", get(server::handlers::metrics_handler))
        .route("/health", get(server::health::health_handler))
        .route("/ready", get(server::health::ready_handler))
        .with_state(state.clone())
        .merge(operations_routes)
        .merge(execution_routes);

    // 4. Apply global layers to the COMPLETE router
    let app = app
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(middleware::request_id::request_id_middleware))
        .layer(axum::middleware::from_fn(middleware::auth::auth_middleware))
        .layer(axum::Extension(auth_config))
        .layer(crate::middleware::cors::cors_layer_from_config(&cors_config));

    // 5. Apply rate limiting if enabled
    let app = if rate_limiting_enabled {
        let limiter = middleware::rate_limit::RateLimiter::new(rate_limiting_config);
        limiter.start_cleanup();
        app
            .layer(axum::middleware::from_fn(middleware::rate_limit::rate_limit_middleware))
            .layer(axum::Extension(limiter))
    } else {
        app
    };
```

### 5.2 Verification Test Additions

An integration test should be added in `tests/security.rs` to verify that unauthenticated requests to `/v1/executions` and `/v1/operations/*` return HTTP 401 when `auth.enabled = true`:

```rust
#[tokio::test]
async fn test_v1_routes_require_authentication() {
    // Construct router using fixed ordering with AuthConfig { enabled: true, api_keys: vec!["valid-key".into()] }
    // Send unauthenticated requests to:
    // - POST /v1/executions
    // - GET /v1/operations/registry
    // Verify both return StatusCode::UNAUTHORIZED (401).
}
```

---

## 6. Conclusion

Refactoring `src/main.rs` to merge `operations_routes` and `execution_routes` into `app` before applying `.layer(auth_middleware)` and `.layer(Extension(auth_config))` completely resolves Requirement R1. It ensures all `/v1/*` endpoints enforce API key authentication when `auth.enabled = true`, while maintaining unauthenticated access for `/health`, `/ready`, and `/metrics`.
