# FusionRouter v0.11.0 Event-Driven Engine & Release Platform

[![Version](https://img.shields.io/badge/version-0.11.0-blue.svg)](https://github.com/NeutronZero/fusion-router/releases/tag/v0.11.0)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-337%20passed-success.svg)](https://github.com/NeutronZero/fusion-router)
[![Architecture](https://img.shields.io/badge/architecture-v0.11.0%20frozen-purple.svg)](docs/fusionrouter_architecture_v0.11.0.md)

An **LLM orchestration operating system, event-driven runtime, and release-governed capability platform** with an immutable event stream ABI (ADR-017), projection-based observability, signed release attestation envelopes, compiler-driven workflows, and multi-channel ingress.

For the full design specification, see [FusionRouter v0.11.0 Architecture Specification](docs/fusionrouter_architecture_v0.11.0.md).

---

## Key Features in v0.11.0

### Event-Driven Runtime Substrate (v0.11 / ADR-017)
- **Runtime Event Stream ABI (ADR-017)**: Canonical observability contract between execution engine and projections.
- **`ExecutionEventEnvelope`**: Schema-versioned (`"fusion.router.event.v1"`) event envelopes carrying `event_id`, `workflow_id`, `execution_id`, `correlation_id`, `sequence_number`, `timestamp`, and typed `ExecutionEvent` payloads.
- **Abstract `EventBus` Trait & `BroadcastEventBus`**: Async broadcast publish/subscribe event engine.
- **`ProjectionDispatcher` Framework**: Decoupled background task dispatch with panic isolation, guaranteeing zero interference with core execution loops.

### Runtime Projections & CLI Tracing
- **OpenTelemetry Projection**: Span & trace mapping exporter (`otel.rs`).
- **Timeline Visualizer**: `TimelineProjection` & `TimelineModel` rendering millisecond-accurate ASCII and JSON execution timelines.
- **Policy-Driven Checkpoint Engine**: `CheckpointProjection` supporting `EveryNode`, `EveryNthNode`, `Timed`, and `Manual` policies with idempotent snapshotting.
- **Persistent Event Store**: `PersistentEventStoreProjection` with append-only JSONL log storage and ordered retrieval.
- **CLI Tracing**: `fusion trace timeline <EXEC_ID> [--format text|json]` and `fusion trace events <EXEC_ID> [--format text|json]`.

### Release Governance Subsystem (Epic M)
- **8 Deterministic Gates**: SemVer (`SDK-1`), Replay Compatibility (`RPL-1`), Upgrade Compatibility (`UPG-1`), Determinism (`DET-1`), Plugin Certification (`PLG-1`), Strategy Certification (`STR-1`), Provider Certification (`PRV-1`), Connector Certification (`CON-1`).
- **Release Policy Engine**: Environment-scoped policy (`policy.yaml`) and waiver evaluation (`waivers.yaml`).
- **Signed Attestations & Archive**: `AttestationBuilder` (sole canonical serialization authority), `Signer` (Mock & Ed25519), `AttestationEnvelope`, and append-only `FilesystemArchiveBackend` (`.fusion/attestations/*.json`).
- **4-Phase Verifier**: `AttestationVerifier` checking Schema Validation $\to$ Canonical Serialization $\to$ Cryptographic Signature $\to$ Semantic Consistency.

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
               Intent Extraction
                        │
                        ▼
              Planner / Compiler
                        │
                        ▼
             Execution Runtime Engine
                        │
                        ▼ (emits ExecutionEventEnvelope)
             Runtime Event Stream ABI (ADR-017)
                        │
                        ▼
              Projection Dispatcher
         ┌────────┬────────┬────────┬────────┐
         ▼        ▼        ▼        ▼        ▼
      OTel    Timeline  Checkpoint Storage  Memory
                        │
                        ▼
              Release Governance (Epic M)
                        │
                        ▼
           Assessment → Attestation
                        │
                        ▼
          Signed Archive (.fusion/attestations/*.json)
```

---

## Test Suite & Verification

FusionRouter v0.11.0 includes **337 test cases** with 0 failures:

```text
lib / integration tests (src/, tests/)   : 332 passed
runtime_events_tests integration test    :   1 passed
cli binary tests (fusion trace/gates)    :   4 passed
----------------------------------------------------------------
Total                                    : 337 passed, 0 failed
```

---

## Documentation

- [System Architecture Specification (v0.11.0)](docs/fusionrouter_architecture_v0.11.0.md)
- [ADR-017: Runtime Event Stream ABI & Observability Substrate](docs/adrs/adr-017-runtime-event-stream-abi.md)
- [ADR-018: Capability Binary Interface (ABI)](docs/adrs/adr-018-capability-binary-interface.md)
- [Quickstart Guide](QUICKSTART.md)
- [Operator Deployment Guide](docs/operator/deployment-guide.md)

---

## License

Dual-licensed under MIT or Apache 2.0.
