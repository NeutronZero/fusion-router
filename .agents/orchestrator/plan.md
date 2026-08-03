# Orchestrator Master Plan: fusion-router

## Overview
Resolution of security vulnerabilities, auth bypasses, rate limiter busy loops, clippy warnings, and comprehensive test coverage in fusion-router.

## Process & Strategy (Project Pattern)
1. **Phase 0: Survey**
   Dispatch 3 parallel Explorers to investigate:
   - Explorer 1: Authentication & Access Control (R1) - route definitions in `src/main.rs`, auth middleware in `src/middleware/auth.rs`, auth configuration, unauthenticated endpoint behaviors.
   - Explorer 2: Shell Command Hardening (R2) & Rate Limiter Guard (R3) - `ShellCommandTool` implementation, command validation/sanitization, rate limiter `start_cleanup` loop in `src/middleware/rate_limit.rs`.
   - Explorer 3: Code Quality, Clippy Warnings & Test Suite (R4) - full build & clippy warning audit, current test coverage, integration tests for `/v1/executions`.

2. **Phase 1: Synthesis & Decomposition**
   Aggregate Explorer reports into `c:\Projects\fusion-router\PROJECT.md`.
   Formulate detailed milestone requirements, interface contracts, and module boundaries.

3. **Phase 2: Milestone Execution**
   For each milestone, execute via Explorer -> Worker -> Reviewer -> Challenger -> Auditor cycle.
   Enforce gate verification rules strictly.

4. **Phase 3: Final Acceptance Verification & Victory Audit**
   Verify:
   - `cargo check --all-targets` (0 errors)
   - `cargo clippy --all-targets --all-features -- -D warnings` (0 warnings)
   - `cargo test --all-features` (all pass)
   - HTTP 401 response for unauthenticated `/v1/executions` when `auth.enabled = true`
   Report completion to Sentinel.
