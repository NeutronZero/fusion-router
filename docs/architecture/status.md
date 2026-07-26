# FusionRouter Architecture Status

> **Purpose:** Single entry point for contributors to understand the current project state.
> **Last updated:** 2026-07-26

---

## Versions

| Artifact | Version | Status |
|----------|---------|--------|
| Architecture | v0.10 | Frozen — changes require ADR |
| Roadmap | v0.11 | Frozen — planning phase complete |
| Implementation stage | Stage 1 (Foundation) | Next |

---

## Platform Invariants

The following invariants are **release-blocking gates** from v0.11 onward (see [Release Gate Specification](../release/release_gate_spec.md)):

1. **Public SDK compatibility** — no silent breaking changes
2. **Replay compatibility** — N → N+1 snapshot replay
3. **Session migration safety** — no data loss across versions
4. **Deterministic compilation** — identical input → identical graph
5. **Stable execution semantics** — golden output stability
6. **Connector conformance** — certified connectors pass on every release
7. **Policy determinism** — identical evaluation for identical inputs
8. **Upgrade/rollback safety** — zero-data-loss upgrade, reversible

---

## Active Implementation Stage

**Stage 1 — Platform Foundation** includes:

| Epic | Area |
|------|------|
| G | Live Configuration |
| B | Streaming Runtime & Metering |
| D | Connector Runtime Platform |
| K | Reliability Engineering |
| M | Compatibility & Release Engineering |
| SDK | Validation Suite (certification tooling) |

Subsequent stages: Distributed Runtime → Intelligence → Platform UX → Enterprise.

---

## Ratified ADRs (31 total)

| Range | Focus |
|-------|-------|
| ADR-001–015 | Foundation, planner, compiler, scheduler, provider, DAG, error handling, telemetry, config, plugins, testing, security, workflow registry |
| ADR-016–020 | Intent-oriented execution, runtime ABI, Strategy SDK, graph alignment, optimization framework |
| ADR-021–027 | Capability platform, plugin ABI, capability resolution, policy compilation, connectors, execution sessions, compiler phase invariants |
| ADR-028–031 | Capability contract evolution, execution semantics, session replay, trigger requests |

Full listing: [`docs/adr/`](../adr/)

---

## Open ADRs

None currently. All ADRs are ratified.

---

## Deferred Architectural Proposals

Tracked in the [Architecture Debt Register](architecture_debt_register.md). Current deferred items:

| ID | Area | Target |
|----|------|--------|
| AD-001 | Out-of-process plugins | v0.11.0 |
| AD-002 | Fine-grained WASM permissions | v0.11.0 |
| AD-003 | Connector load balancing | v0.11.0 / v1.0.0 |
| AD-004 | Distributed capability cache | v1.0.0 |

---

## Key Documents

| Document | Location |
|----------|----------|
| Architecture Specification | [`specification.md`](specification.md) |
| Architectural Invariants | [`invariants.md`](invariants.md) |
| Architecture Debt Register | [`architecture_debt_register.md`](architecture_debt_register.md) |
| v0.11 Roadmap | [`../roadmap-v0.11.md`](../roadmap-v0.11.md) |
| Release Gate Specification | [`../release/release_gate_spec.md`](../release/release_gate_spec.md) |
| ADR-027 Compiler Phase Invariants | [`../adr/ADR-027-compiler-phase-invariants.md`](../adr/ADR-027-compiler-phase-invariants.md) |
