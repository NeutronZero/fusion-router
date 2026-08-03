# ADR-035: Fail-Closed Deployment

- **Status:** Draft
- **Date:** 2026-08-03
- **Applies to:** configuration (`src/config`), server bootstrap (`src/main.rs`), middleware (`src/middleware`)
- **Charter:** `docs/implementation/security-hardening-v0.13.1.md` Phase 2, Runtime Law 6
- **Amends:** ADR-012 (Security Model)

## Context

ADR-012 declared authentication, rate limiting, and CORS restriction as opt-in. The security audit (findings C1, M2, M3) shows the default posture is fail-open: auth disabled, server bound to `0.0.0.0`, CORS `*`, rate limiting disabled, and `cat`/`ls`/`http_request` tools enabled by default. A default `cargo run` is a public execution gateway. ADR-012's "opt-in" security model is the root cause.

## Decision

1. **Fail-closed defaults (release builds):** default host `127.0.0.1`; `auth.enabled: true`; rate limiting enabled; CORS same-origin only (no `*`); shell and HTTP tools disabled. `validate()` rejects insecure configurations in release builds.
2. **Explicit escape hatch:** `--unsafe-dev` flag restores the historical fail-open behavior for development only; each insecure setting it permits logs a prominent warning. Placeholder API keys remain gated behind it.
3. **Identity-based rate limiting:** bucket keys derive from the unspoofable peer address (`ConnectInfo`), or from `x-forwarded-for` only when a trusted-proxy list is configured; bucket count is bounded.
4. **Constant-time credential checks:** API keys compared via constant-time comparison over digests; key length bounds enforced.

## Consequences

- A default install is unreachable without authentication, bound to loopback, rate-limited, with no dangerous tools registered.
- Operators who want the previous posture must explicitly pass `--unsafe-dev`; CI/operator docs updated (`docs/operator/*`).
- ADR-012 sections 1, 3, 4 (opt-in auth/CORS/rate-limit semantics) are superseded by this ADR where they conflict; ADR-012 is amended, not rescinded.
