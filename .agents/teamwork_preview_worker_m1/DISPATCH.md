## 2026-08-03T10:33:15Z

<USER_REQUEST>
You are teamwork_preview_worker instance for Milestone 1: Access Control & Authentication Middleware.
Working directory: c:\Projects\fusion-router\.agents\teamwork_preview_worker_m1
Original Request Path: c:\Projects\fusion-router\.agents\ORIGINAL_REQUEST.md
Survey Analysis Path: c:\Projects\fusion-router\.agents\teamwork_preview_explorer_survey_1\handoff.md

Your task:
Implement Requirement R1 (Access Control & Authentication Middleware):
1. Read ORIGINAL_REQUEST.md and the survey handoff report at c:\Projects\fusion-router\.agents\teamwork_preview_explorer_survey_1\handoff.md.
2. Modify `src/main.rs`:
   - Reorder router setup so that `operations_routes` and `execution_routes` are merged into `app` BEFORE chaining `.layer(axum::middleware::from_fn(middleware::auth::auth_middleware))` and `.layer(axum::Extension(auth_config))`.
   - Ensure all `/v1/*` routes (specifically including `/v1/executions` and `/v1/operations/*`) pass through `auth_middleware`.
3. Add integration tests in `tests/integration/opencode.rs` or `tests/security.rs`:
   - Verify that when `auth.enabled = true`, unauthenticated requests to `POST /v1/executions` return HTTP 401 Unauthorized.
   - Verify that unauthenticated requests to `/v1/operations/*` return HTTP 401 Unauthorized.
   - Verify that valid API key requests to `/v1/executions` pass auth.
4. Run `cargo check --all-targets` and `cargo test --all-features`. Ensure all tests pass.
5. Create your working directory at `c:\Projects\fusion-router\.agents\teamwork_preview_worker_m1`. Write your implementation report to `c:\Projects\fusion-router\.agents\teamwork_preview_worker_m1\handoff.md` including exact diffs, command lines executed, and build/test outputs.

MANDATORY INTEGRITY WARNING: DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.
</USER_REQUEST>
