# FusionRouter v0.13.1 — AI Execution Compiler & Security-Hardened Runtime

[![Version](https://img.shields.io/badge/version-0.13.1-blue.svg)](https://github.com/NeutronZero/fusion-router/releases/tag/v0.13.1)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-1421%20passed-success.svg)](https://github.com/NeutronZero/fusion-router)
[![Architecture](https://img.shields.io/badge/architecture-v0.13.0%20frozen-purple.svg)](docs/specifications/architecture-v0.13.md)

FusionRouter is an **AI execution compiler**: it compiles high-level user intent into executable, verifiable, replayable workflow artifacts — exactly as a traditional compiler turns source code into machine code. Execution is a compilation product, never an improvisation.

For the frozen v0.13.0 architecture specification, see [Architecture Specification (v0.13.0)](docs/specifications/architecture-v0.13.md). For the v0.13.1 security hardening milestone, see the [v0.13.1 Charter](docs/implementation/security-hardening-v0.13.1.md).

---

## Key Features in v0.13.x

### Six Frozen Core Abstractions (v0.13.0 / ADR-032, ADR-033)
- **`NormalizedIntent`** — canonical, provider-free user goals and constraints (Contract 1).
- **`WorkflowIR`** — versioned, immutable logical workflow graph (Contract 2).
- **`ExecutionAbi`** — executable workflow contract between compiler and runtime; only the compiler generates it (Contract 3).
- **`ExecutionTarget`** — provider-independent runtime placement and environment constraints (Contract 4).
- **`ExecutionRuntimeInterface` (ERI)** — stable runtime execution contract; 9-state execution model (Contract 5).
- **`CapabilityRegistry` + `CapabilityTrait`** — semantic capability catalog with execution-relevant traits (Contract 6).

### Security Hardening (v0.13.1 / ADRs 034–037)
- **Fail-closed deployment (Phase 2 / ADR-035)** — release builds reject auth-off or rate-limit-off configurations; `--unsafe-dev` is the only escape hatch (Law 6).
- **Tool Execution Trust Boundary (Phase 3 / ADR-037, Law 7)** — model output is never interpreted as executable actions:
  - Executor consumes **provider-native `tool_calls` only**; free-form JSON tool parsing removed.
  - Per-request `tool_allowlist` + `allow_auto_exec` (default **false**) gate every tool execution.
  - **Shell tool argument policy** — command allowlist and allowed read directories; unrestricted arguments disabled by default.
  - **HTTP tool URL policy** — HTTPS-only scheme enforcement plus SSRF defense (private/loopback/link-local blocklist with DNS recheck).
- **Compiler contract enforcement (Phase 1)** — single `build_compiler()` factory; policy `Deny` enforced at compile time on all resolution paths.
- Milestone laws verified by `tests/security_invariants.rs` (law1–law10 scaffold per charter).

### Release Governance & Platform (v0.11–v0.13 foundation)
- **Runtime Event Stream ABI (ADR-017)** — schema-versioned `ExecutionEventEnvelope` events, `BroadcastEventBus`, `ProjectionDispatcher` with panic isolation.
- **Release Governance** — 8 deterministic gates, policy engine, signed attestations (`AttestationBuilder`, Ed25519 `Signer`, 4-phase `AttestationVerifier`), append-only attestation archive.
- **Capability Platform** — `.fusionpkg` package format (verify → load → resolve → execute), WASM sandboxing (fuel/memory limits), host services, `fusion` CLI (`new`, `build`, `test`, `publish`, `dev`).
- **Operations Platform** — `/v1/operations/*` REST API (dashboard data, runtime inspector, policy admin, attestation viewer).
- **Live Configuration** — two-phase config reload with generation counter, provider/connector subscribers.

---

## Quick Start

### Prerequisites
- Rust 1.75+ (2021 Edition)

### Build & Run
```bash
# Run local dev server (default port 8080)
cargo run

# Run all tests
cargo test

# Run all tests including optional features (semantic-cache, prometheus-metrics)
cargo test --all-features

# Evaluate release policy & attest release
cargo run --bin fusion -- gates evaluate --env production
cargo run --bin fusion -- gates attest --env production

# Trace execution timeline
cargo run --bin fusion -- trace timeline exec-123 --format text
```

---

## System Architecture Pipeline

```text
                 Unified Ingress
                        │
                        ▼
        Intent Normalization (NormalizedIntent)
                        │
                        ▼
      Planner / Compiler Pipeline (WorkflowIR → ExecutionAbi)
                        │
                        ▼
       Execution Runtime Engine (ERI, 9-state model)
                        │
                        ▼ (emits ExecutionEventEnvelope)
        Runtime Event Stream ABI (ADR-017)
                        │
                        ▼
         Projection Dispatcher (panic-isolated)
   ┌────────┬────────┬────────┬────────┬────────┐
   ▼        ▼        ▼        ▼        ▼        ▼
 OTel   Timeline  Checkpoint Storage    Memory  Evidence
                        │
                        ▼
        Release Governance (8 gates)
                        │
                        ▼
   Assessment → Signed Attestation Archive
```

---

## Test Suite & Verification

FusionRouter v0.13.1 passes **1421 tests** (all features) with 0 failures:

```text
lib unit tests (src/)                       : 667 passed
main binary unit tests (src/main.rs)        : 569 passed
cli binary tests (src/bin/fusion.rs)        :  10 passed
integration tests (tests/*.rs)              : 175 passed
---------------------------------------------------------------
Total (cargo test --all-features)           : 1421 passed, 0 failed
```

Includes `tests/security_invariants.rs` (milestone law tests) and live-validated fail-closed behavior for auth, tool allowlist, shell argument policy, and SSRF protection.

---

## Documentation

- [Architecture Specification (v0.13.0, frozen)](docs/specifications/architecture-v0.13.md)
- [v0.13.1 Security Hardening Charter](docs/implementation/security-hardening-v0.13.1.md)
- [ADR-032: Execution ABI separate from PrimitiveGraph](docs/adrs/adr-032-execution-abi-separate-from-primitivegraph.md)
- [ADR-033: Architecture Freeze](docs/adrs/adr-033-architecture-freeze.md)
- [ADR-034: Single Compiler Pipeline](docs/adrs/adr-034-single-compiler-pipeline.md)
- [ADR-035: Fail-Closed Deployment](docs/adrs/adr-035-fail-closed-deployment.md)
- [ADR-036: Plugin Execution Context](docs/adrs/adr-036-plugin-execution-context.md)
- [ADR-037: Structured Tool Invocation](docs/adrs/adr-037-structured-tool-invocation.md)
- [Provider API Specification (tool-call contract)](docs/specifications/provider-api.md)
- [Quickstart Guide](QUICKSTART.md)
- [Operator Deployment Guide](docs/operator/deployment-guide.md)

---

## License

Dual-licensed under MIT or Apache 2.0.
