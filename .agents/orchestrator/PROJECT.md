# Project: fusion-router

## Overview
Comprehensive remediation of access control bypasses, shell command execution vulnerabilities, rate limiter CPU busy-spin loops, clippy lints, and compiler warnings in `fusion-router`.

## Architecture & Scope
- **Module Boundaries**:
  - `src/main.rs`, `src/middleware/auth.rs`: HTTP router stack, middleware scoping, route composition.
  - `src/tools/shell_tool.rs`, `src/config/mod.rs`, `config/default.yaml`: Tool invocation, shell interpreter restrictions, default command whitelist.
  - `src/middleware/rate_limit.rs`: Rate limiting middleware, cleanup loop interval logic.
  - Repository-wide (`src/` and `tests/`): Clippy lints, dead code compiler warnings, unit & integration test coverage.

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | F1. Auth Middleware Scoping | Merge `execution_routes` & `operations_routes` before `auth_middleware` layer in `src/main.rs` | M1 | R1 |
| 2 | F2. Executions & Operations Auth | Enforce API key auth on `/v1/executions` and `/v1/operations/*` when `auth.enabled = true` | M1 | R1 |
| 3 | F3. Auth Integration Tests | Add integration tests verifying HTTP 401 on unauthenticated `/v1/executions` & `/v1/operations/*` | M1 | R1 |
| 4 | F4. Shell Tool Hardening | Reject shell interpreter binaries (`cmd`, `sh`, `bash`, `powershell`, etc.) in `ShellCommandTool` | M2 | R2 |
| 5 | F5. Config Whitelist Cleanup | Remove `"cmd"` from default allowed shell commands in `src/config/mod.rs` & `config/default.yaml` | M2 | R2 |
| 6 | F6. Shell Security Tests | Update security tests to verify shell injection prevention and rejection of `"cmd"` | M2 | R2 |
| 7 | F7. Rate Limiter Floor | Enforce non-zero minimum interval floor (`cleanup_interval_secs.max(1)`) in `RateLimiter::start_cleanup` | M2 | R3 |
| 8 | F8. Rate Limiter Unit Test | Add unit test verifying non-zero cleanup interval enforcement under zero input config | M2 | R3 |
| 9 | F9. Clippy Lints Resolution | Fix all 31 clippy warnings across 18 source files so `cargo clippy` passes cleanly with `-D warnings` | M3 | R4 |
| 10 | F10. Dead Code Warning Cleanup | Address 164 compiler warnings (unused imports, unused functions, unread fields) | M3 | R4 |
| 11 | F11. E2E Acceptance Verification | Verify `cargo check`, `cargo clippy`, `cargo test`, and unauthenticated `/v1/executions` HTTP 401 | M4 | AC 1-4 |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | Access Control & Auth Middleware | Fix router merge ordering in `src/main.rs` and add integration tests for `/v1/executions` & `/v1/operations/*` auth | None | PLANNED |
| M2 | Shell Hardening & Rate Limiter Guard | Reject shell binaries in `ShellCommandTool`, remove `"cmd"` from defaults, clamp `start_cleanup` interval floor | None | PLANNED |
| M3 | Code Quality & Clippy Warnings | Resolve 31 clippy errors across 18 files and resolve dead code warnings | None | PLANNED |
| M4 | Final E2E Acceptance Verification | Verify `cargo check`, `cargo clippy`, `cargo test`, and HTTP 401 on `/v1/executions` | M1, M2, M3 | PLANNED |

## Interface Contracts & Guidelines
- `auth_middleware`: Fail-closed behavior (returns HTTP 401 if auth header invalid/missing when `auth.enabled = true`). Whitelists only `/health`, `/ready`, `/metrics`.
- `ShellCommandTool`: `validate_command` must fail for any shell interpreter binary regardless of `allowed_commands` content.
- `RateLimiter::start_cleanup`: `Duration` used in `sleep` must be at least 1 second (`.max(1)`).

## Code Layout
- `src/main.rs`
- `src/middleware/auth.rs`
- `src/middleware/rate_limit.rs`
- `src/tools/shell_tool.rs`
- `src/config/mod.rs`
- `config/default.yaml`
- `tests/integration/opencode.rs`
- `tests/security.rs`
