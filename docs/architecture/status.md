# FusionRouter Architecture Status

> **Purpose:** Single entry point for contributors to understand the current project state.
> **Last updated:** 2026-08-24

---

## Versions

| Artifact | Version | Status |
|----------|---------|--------|
| Architecture | v0.14.5 LTS Foundation | Frozen — changes require ADR |
| Roadmap | v0.15 Distributed Architecture | Active — implementation in progress |
| Implementation stage | Stage 2 (Distributed Runtime & Scaling) | Active |

---

## Platform Invariants

The following 11 invariants are **release-blocking gates** (see [Release Gate Specification](../release/release_gate_spec.md)):

1. **Public SDK compatibility** — no silent breaking changes
2. **Replay compatibility** — N → N+1 snapshot replay & 100% replay fidelity
3. **Session migration safety** — no data loss across versions
4. **Deterministic compilation** — identical input → identical graph
5. **Stable execution semantics** — golden output stability
6. **Connector conformance** — certified connectors pass on every release
7. **Policy determinism** — identical evaluation for identical inputs
8. **Upgrade/rollback safety** — zero-data-loss upgrade, reversible
9. **Zero-bypass governance** — 100% compiler invocation rate (AF-003 Law 1)
10. **Fail-closed deployment** — release validation rejects insecure configurations (ADR-035)
11. **Certified Performance SLOs** — Planner <10ms, Compiler <20ms, Scheduler <5ms, Runtime Overhead <10ms, Replay <20ms

---

## Active Implementation Stage

**Stage 2 — Distributed Architecture & Scaling** includes:

| Epic | Area |
|------|------|
| D-DIST | Distributed Scheduler & Remote Worker Protocol |
| C-CAP | Capability Federation & Registry Mirroring |
| R-METER | Real-Time Metering & Resource Budgeting |
| SEC-REL | Fail-Closed Policy Enforcement & Replay Attestation |

---

## 2026-08-24 Hardening & De-phantom Pass

Enforcement, streaming, and operational-honesty changes (all behaviorally tested):

- **Runtime policy enforcement is wired.** The compiler's `policy` pass is now attached per
  request from the live `PolicyRegistry` snapshot (previously every production call site passed
  `None`, and the chat pipeline downgraded policies to audit-only). Deny rules created via the
  admin API block compilation with **403 `PolicyDenied`** citing the rule; malformed registry
  entries fail closed.
- **Native upstream streaming** for single-node graphs: chunks flow through `MeteredStream`
  (mid-stream budget-breach termination; client disconnect releases the reservation and books
  actual usage). Orchestrated graphs keep the re-chunked SSE transport (`x-fusion-stream-mode`
  header distinguishes them).
- **Live reload**: SIGHUP now hot-applies API keys/scopes and rate-limit settings in addition to
  providers/connectors. Keys support an `operator` scope tier required for `/v1/operations/*`,
  `/v1/executions`, and `/metrics`.
- **Phantom subsystems removed.** Crates deleted: `fusion-worker`, `fusion-worker-protocol`,
  `fusion-api-public`, `fusion-api-internal` (hardcoded replay engine), `fusion-infrastructure`,
  `fusion-placement` (constant-score placement), `fusion-security`. Preserved pieces:
  AES-GCM `SecretManager` → `src/security/secrets.rs`; `ExecutionLeaseManager` →
  `fusion_scheduler::leases`. Dead monolith modules removed (ERI/ABI stack, trigger engine,
  package loader, capability resolver, sandbox-adjacent orphans); ~10k LOC net reduction.
- **Operational fixes**: planner failures return retryable 503 (no panic); `/ready` performs a
  real DB ping + provider check; evidence snapshot cached (30s TTL) with a 7-day retention job;
  server-wide request timeout (300s default) and concurrency envelope enforce
  `resources.max_concurrent`; release builds refuse to boot when a provider key is unresolvable.

ADR-006 and ADR-040 remain historical records; their reference implementations never existed as
specified and were removed rather than half-wired.

---

## Ratified ADRs (38 total)

| Range | Focus |
|-------|-------|
| ADR-001–015 | Foundation, planner, compiler, scheduler, provider, DAG, error handling, telemetry, config, plugins, testing, security, workflow registry |
| ADR-016–020 | Intent-oriented execution, runtime ABI, Strategy SDK, graph alignment, optimization framework |
| ADR-021–027 | Capability platform, plugin ABI, capability resolution, policy compilation, connectors, execution sessions, compiler phase invariants |
| ADR-028–031 | Capability contract evolution, execution semantics, session replay, trigger requests |
| ADR-032–033 | Execution ABI v1 separation, v0.13 architecture freeze declaration |
| ADR-034–037 | Compiler pass registration, fail-closed production deployment, tool-execution trust boundary |
| ADR-038 | Multi-model ensemble review & remediation CLI |

Full listing: [`docs/adr/`](../adr/)

---

## Open ADRs

None currently. All 38 ADRs are ratified.

---

## Key Documents

| Document | Location |
|----------|----------|
| Architecture Specification | [`specification.md`](specification.md) |
| Architecture Handbook | [`../developer/handbook.md`](../developer/handbook.md) |
| Implementation Roadmap | [`../implementation/roadmap.md`](../implementation/roadmap.md) |
| Release Gate Specification | [`../release/release_gate_spec.md`](../release/release_gate_spec.md) |
| v1.0 Readiness Report | [`../governance/v1-readiness-report.md`](../governance/v1-readiness-report.md) |
