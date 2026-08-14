# ADR-012: Security Model

## Status
Accepted — **Amended by ADR-035 (Fail-Closed Deployment, 2026-08-03)**

> Amendment: Sections 1 (opt-in auth), 3 (CORS defaults), and 4 (opt-in rate limiting) are superseded where they conflict with ADR-035. The security model moves from opt-in to fail-closed: authentication, rate limiting, and same-origin CORS are default-enabled in release builds; insecure postures require the explicit `--unsafe-dev` flag.

## Context
FusionRouter exposes an HTTP API for LLM routing, which may be deployed in production environments requiring access control, origin restrictions, and abuse prevention. Early versions had no security — the server was open to any client. As adoption grows, operators need opt-in authentication, cross-origin request control, and rate limiting to protect against unauthorized use and resource exhaustion.

## Decision

### 1. API Key Authentication (Opt-In)

Auth is disabled by default and enabled via `auth.enabled: true` in config:
- Clients present their API key in the `x-api-key` HTTP header
- Valid keys are configured in `auth.api_keys` (list of strings)
- Unauthenticated requests receive HTTP 401 with JSON body `{"error": "unauthorized"}`
- Auth is implemented as an axum middleware in `src/middleware/auth.rs`
- Compatible with the OpenAI API key convention (`x-api-key` header)

### 2. Whitelisted Paths

When auth is enabled, the following paths bypass authentication:
- `/health` — health check endpoint
- `/ready` — readiness check
- `/metrics` — Prometheus metrics endpoint

This ensures monitoring systems can access the server without requiring API keys.

### 3. CORS Middleware

Configurable CORS via `server.cors` in config:
- `allowed_origins` — list of allowed origins (default `["*"]` for wide access)
- `allowed_methods` — list of allowed HTTP methods (default standard REST methods + OPTIONS)
- `allowed_headers` — list of allowed headers (default: `content-type`, `authorization`, `x-api-key`, `x-request-id`)
- Wildcard origin (`*`) triggers `AllowOrigin::any()`; specific origins use `AllowOrigin::list()`
- Empty method/header lists disable method/header filtering

CORS layer is built using `tower_http::cors::CorsLayer` via the `cors` feature of `tower-http`.

### 4. Token Bucket Rate Limiting

Rate limiting is opt-in via `rate_limiting.enabled: true`:
- Per-client token bucket using `dashmap::DashMap` for concurrent access
- Clients identified by `x-api-key` header, falling back to `x-forwarded-for`, then `"unknown"`
- Configurable `requests_per_minute` (default 60) and `burst_size` (default 10)
- Buckets refill continuously at the configured rate
- Exceeded requests receive HTTP 429 with `{"error": "rate_limit_exceeded", "retry_after_secs": N}`
- Background cleanup task periodically evicts stale buckets (configurable `cleanup_interval_secs`)
- Rate-limited paths excluded: `/health`, `/ready`, `/metrics`

### 5. Middleware Ordering

Middleware stack is ordered in `main.rs` as:
1. CORS (outermost — apply before any request processing)
2. Auth (authenticate before inspecting the request)
3. Request ID (generate UUID before logging)
4. Rate Limiting (check limits after authentication)
5. Handler

### 6. Provider API Key Management

Upstream provider API keys are supplied via environment variables:
- `OPENCODEZEN_API_KEY` — OpenCodeZen provider
- `OPENROUTER_API_KEY` — OpenRouter provider
- Keys are passed as `Authorization: Bearer` headers to upstream services
- Provider config includes `api_key_env` for per-provider key customization

### 7. Resource Protection

Beyond rate limiting, the resource manager enforces:
- Daily cost and token budgets (global and per-provider)
- Maximum concurrent request limits
- Per-request budget envelopes (cost, tokens, iteration caps)
- `ResourceGuard` with Drop-based release semantics prevents quota leaks

## Consequences

- Production deployments can opt into authentication without code changes
- CORS configuration supports both open (wildcard) and restricted origins
- Rate limiting protects against abuse with configurable thresholds
- Provider API keys stay out of config files and config management systems
- Middleware ordering ensures consistent policy application
- Monitoring paths bypass auth, keeping ops tooling unauthenticated
- Resource protections act as a second line of defense beyond rate limiting
