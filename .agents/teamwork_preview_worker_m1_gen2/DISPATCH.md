# Dispatch Assignment: Milestone 1 (Access Control & Auth Middleware)

## Mission
Fix authentication bypass in `src/main.rs` and add integration tests verifying HTTP 401 on unauthenticated `/v1/executions` and `/v1/operations/*` when `auth.enabled = true`.

## Target Files
- `src/main.rs`
- `tests/integration/opencode.rs` and/or `tests/security.rs`

## Detailed Instructions
1. Read `c:\Projects\fusion-router\.agents\ORIGINAL_REQUEST.md` and survey report `c:\Projects\fusion-router\.agents\teamwork_preview_explorer_survey_1\handoff.md`.
2. In `src/main.rs`:
   - Reorder router assembly so `execution_routes` (`/v1/executions`) and `operations_routes` (`/v1/operations/*`) are merged into `app` BEFORE chaining `.layer(axum::middleware::from_fn(middleware::auth::auth_middleware))` and `.layer(axum::Extension(auth_config))`.
3. Add integration test(s) in `tests/integration/opencode.rs` or `tests/security.rs`:
   - Verify that when `auth.enabled = true`, unauthenticated requests to `POST /v1/executions` return HTTP 401 Unauthorized.
   - Verify that unauthenticated requests to `GET /v1/operations/registry` return HTTP 401 Unauthorized.
   - Verify that valid API key requests pass authentication.
4. Run `cargo check --all-targets` and `cargo test --all-features`.
5. Write `handoff.md` in your working directory `c:\Projects\fusion-router\.agents\teamwork_preview_worker_m1_gen2` detailing changes, test commands, and output.

## MANDATORY INTEGRITY WARNING
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.
