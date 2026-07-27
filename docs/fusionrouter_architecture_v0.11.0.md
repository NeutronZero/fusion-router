# FusionRouter v0.11.0 Event-Driven Engine & Release Platform — System Architecture Specification & Engineering Reference

> **Classification**: Production-Grade Technical Architecture Document
> **Version**: 0.11.0 | **Edition**: Rust 2021 | **Date**: 2026-07-27
> **Repository**: `fusion-router` | **Governance**: ADR-007 through ADR-031, ADR-017 (Runtime Event Stream ABI)
> **Status**: ✅ Complete — Release Governance (Epic M1–M5) & Event-Driven Engine (v0.11 N1–N5) verified.

---

## Table of Contents

1. [Executive Overview & Architectural Philosophy](#1-executive-overview--architectural-philosophy)
2. [Full System Architecture Diagram](#2-full-system-architecture-diagram)
3. [Subsystem Engineering Reference](#3-subsystem-engineering-reference)
   - 3.1 [Staged Request Pipeline](#31-staged-request-pipeline)
   - 3.2 [Context Assembly & Safety Mechanics](#32-context-assembly--safety-mechanics)
   - 3.3 [Requirements Extraction & Intent Classification](#33-requirements-extraction--intent-classification)
   - 3.4 [Compiler & DAG Execution Engine](#34-compiler--dag-execution-engine)
   - 3.4.5 [Planning Subsystem](#345-planning-subsystem)
   - 3.5 [Resource Safety & Budget Envelopes](#35-resource-safety--budget-envelopes)
   - 3.6 [Provider Selection & Resilience](#36-provider-selection--resilience)
   - 3.7 [Reasoning Strategies](#37-reasoning-strategies)
   - 3.8 [Closed-Loop Feedback Calibration](#38-closed-loop-feedback-calibration)
   - 3.9 [Semantic Vector Cache](#39-semantic-vector-cache)
   - 3.10 [Sandboxed Extension Engine](#310-sandboxed-extension-engine)
   - 3.11 [Tools & Registry](#311-tools--registry)
   - 3.12 [Telemetry & Observability](#312-telemetry--observability)
   - 3.13 [Artifact Model](#313-artifact-model)
   - 3.14 [Trigger Framework & Unified Ingress](#314-trigger-framework--unified-ingress)
   - 3.15 [Connector Ecosystem](#315-connector-ecosystem)
   - 3.16 [Developer Experience & Diagnostics](#316-developer-experience--diagnostics)
   - 3.17 [Distributed Scheduling](#317-distributed-scheduling)
   - 3.18 [Production Hardening](#318-production-hardening)
   - 3.19 [Session Continuity & Replay](#319-session-continuity--replay)
   - 3.20 [Release Governance Subsystem (Epic M)](#320-release-governance-subsystem-epic-m)
   - 3.21 [Event-Driven Runtime & Projections (v0.11 / ADR-017)](#321-event-driven-runtime--projections-v011--adr-017)
4. [Request Lifecycle Walkthrough](#4-request-lifecycle-walkthrough)
5. [Security, Concurrency & Resilience Matrix](#5-security-concurrency--resilience-matrix)
6. [Workspace Structure & Dependency Mapping](#6-workspace-structure--dependency-mapping)
7. [Exhaustive Architectural Gap Analysis & Resolution Matrix](#7-exhaustive-architectural-gap-analysis--resolution-matrix)

---

## 1. Executive Overview & Architectural Philosophy

### Purpose

FusionRouter is an **intelligent LLM orchestration operating system, event-driven runtime, and release-governed capability platform**. It processes requests through a single unified ingress pipeline (`ExecutionRequest`), dynamic capability resolution (`CapabilityResolver`), declarative policy compilation (`PolicyCompilerPass`), deterministic execution session continuity (`ReplayEngine`), event-sourced runtime projections (`EventProjection`), and cryptographically signed release attestations (`SignedAttestation`).

### Core Architectural Principles & Platform Invariants

1. **Interface-First Architecture:** Core subsystems depend strictly on traits and abstract contracts (`ReleaseGate`, `Signer`, `ArchiveBackend`, `EventBus`, `EventProjection`) rather than concrete implementations.
2. **Immutable System Records:** Execution events, release assessments, and signed attestations are append-only and strictly immutable after publication.
3. **Compiler-Oriented Planning:** Requests express intent and compile into internal `ExecutionGraph` DAGs through a transactional compiler pipeline before execution.
4. **Event-Driven Observability:** Runtime capabilities consume the canonical Event Stream ABI (`ADR-017`) through decoupled projections without executor coupling.
5. **Policy Over Implementation:** Governance decisions, release approvals, waivers, and checkpointing strategies are driven by declarative policies (`policy.yaml`, `waivers.yaml`, `CheckpointPolicy`).
6. **Extensibility Through Composition, Not Modification:** New functionality is introduced by adding projections, providers, or capability connectors without mutating core execution loops.

| Principle | Mechanism |
|-----------|-----------|
| **Event Stream as Runtime ABI (ADR-017)** | All runtime operations emit append-only `ExecutionEventEnvelope` variants; observability, timeline, checkpointing, and storage consume events via projections without executor coupling |
| **Release Governance Pipeline (Epic M)** | 8 deterministic gates (`SDK-1`, `RPL-1`, `UPG-1`, `DET-1`, `PLG-1`, `STR-1`, `PRV-1`, `CON-1`) evaluated against environment policy (`policy.yaml`) and auditable waivers (`waivers.yaml`) |
| **Signed Release Attestations** | `ReleaseAssessment` handoff bundles serialized via `AttestationBuilder` (sole authority), signed via `Signer`, wrapped in `AttestationEnvelope`, and archived in append-only storage |
| **Decoupled Projection Framework** | Projections implement `EventProjection` (`name()`, `handle_event()`); `ProjectionDispatcher` fan-out engine insulates core execution loop from projection panics or delays |
| **Strategy–Provider Decoupling** | `Strategy` trait lowers into `PrimitiveGraph` IR independent of provider identity; `ProviderRouter` resolves physical endpoints at execution time |
| **Compile-Before-Execute** | All `WorkflowIR` passes through a transactional `Compiler` pipeline (constraint → control-flow → model resolution → policy compilation) before execution |
| **Session Continuity & Replay** | `ExecutionSession` identity separated from `SessionSnapshot`; supports `Deterministic`, `Inspection` (side-effect free), and `Simulation` replay modes |
| **RAII Resource Safety** | `ResourceGuard` auto-releases quota on `Drop` if uncommitted; `BudgetEnvelope` enforces per-request cost/token/iteration ceilings via `Arc<AtomicU64>` |

---

## 2. Full System Architecture Diagram

```text
                                INGRESS LAYER
┌─────────────────────────────────────────────────────────────────────────────┐
│  Webhooks  │  Cron Scheduler  │  EventBus Subscriber  │  Manual HTTP API    │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │
                                       ▼
                       ExecutionRequest Ingress Normalization
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           COMPILER & DAG ENGINE                             │
│  IntentPlanner ──► WorkflowIR ──► Transactional Compiler Pipeline          │
│  (Validation ──► ControlFlow ──► ModelResolution ──► PolicyCompilerPass)    │
│                                      │                                      │
│                                      ▼                                      │
│                            ExecutionGraph DAG                               │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                       EVENT-DRIVEN RUNTIME ENGINE                           │
│  DefaultScheduler (buffer_unordered) ──► DefaultExecutor (ResourceGuard)    │
│                                      │                                      │
│                                      ▼ (emits ExecutionEventEnvelope)       │
│                  EventBus Trait (BroadcastEventBus)                         │
│                                      │                                      │
│                                      ▼                                      │
│                        ProjectionDispatcher Fan-Out                         │
│  ┌──────────────┬────────────┼──────────────┬──────────────┬─────────────┐  │
│  ▼              ▼            ▼              ▼              ▼             ▼  │
│ OTel          Timeline   Checkpoint     Persistent      Memory       Audit  │
│ Exporter     Visualizer    Engine       Event Store     Bridge        Log   │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                       RELEASE GOVERNANCE LAYER (EPIC M)                     │
│  GateRunner (8 Gates: SDK-1, RPL-1, UPG-1, DET-1, PLG-1, STR-1, PRV-1, CON-1)│
│                                      │                                      │
│                                      ▼                                      │
│  PolicyEvaluator (policy.yaml + waivers.yaml) ──► ReleaseDecision           │
│                                      │                                      │
│                                      ▼                                      │
│  AttestationBuilder (Sole Authority) ──► Signer ──► AttestationEnvelope     │
│                                      │                                      │
│                                      ▼                                      │
│                 FilesystemArchiveBackend (.fusion/attestations/*.json)      │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Subsystem Engineering Reference

### 3.20 Release Governance Subsystem (Epic M)

#### Module Map (`src/release/`)

- `gate.rs`: `ReleaseGate` trait, `GateResult`, `GateCheck`, `GateMetadata`, `GateId` (`SDK-1`, `RPL-1`, `UPG-1`, `DET-1`, `PLG-1`, `STR-1`, `PRV-1`, `CON-1`).
- `runner.rs`: `GateRunner` executing registered gates in canonical FIFO order.
- `certification.rs`: `CertificationArtifact` trait and `CertificationContext` certifying plugins, strategies, providers, and connectors.
- `policy.rs`: `ReleaseEnvironment` (`Production`, `Staging`, `Development`, `Custom`), `PolicyDefinition`, YAML loader (`policy.yaml`).
- `waiver.rs`: `Waiver`, `WaiverSet` with mandatory stable IDs (`id: waiver-2026-0042`) and expiration timestamp checking (`waivers.yaml`).
- `evaluator.rs`: `EvaluationContext`, `EvidenceClassifier`, `PolicyEvaluator`, `ReleaseDecision` (`Approved`, `ApprovedWithWaivers`, `Blocked`).
- `assessment.rs`: `ReleaseAssessment` bundle with content-derived `assessment_id` (`asm-<hex>`).
- `attestation.rs`: `ReleaseAttestation`, `HostInfo`, and `AttestationBuilder` (sole authority for canonical UTF-8 JSON serialization).
- `signing.rs`: Versioned `SignatureBlock` (version `1`), `SignedAttestation`, `Signer` trait, `MockSigner`, `Ed25519Signer`.
- `envelope.rs`: `AttestationEnvelope` (`envelope_version: "v1"`).
- `archive.rs`: `ArchiveBackend` trait and `FilesystemArchiveBackend` (append-only storage under `.fusion/attestations/*.json`, rejecting overwrites).
- `verifier.rs`: 4-Phase `AttestationVerifier` (*Schema Validation* $\to$ *Canonical Serialization* $\to$ *Cryptographic Verification* $\to$ *Semantic Consistency*).

---

### 3.21 Event-Driven Runtime & Projections (v0.11 / ADR-017)

#### Module Map (`src/events/`)

- `mod.rs`: `ExecutionEventEnvelope` carrying `schema_version: "fusion.router.event.v1"`, `event_id`, `workflow_id`, `execution_id`, `correlation_id`, `sequence_number`, `timestamp`, and `parent_event_id`.
- `payload.rs`: `ExecutionEvent` strongly-typed enum taxonomy (workflow lifecycle, compilation, node execution, retries, provider calls, tool invocations, and resource lifecycle).
- `bus.rs`: Abstract `EventBus` trait (`publish`, `subscribe`) and `BroadcastEventBus` (backed by `tokio::sync::broadcast`).
- `projection.rs`: `EventProjection` trait (`name()`, `handle_event()`) and `ProjectionDispatcher` background fan-out engine with panic isolation.
- `consumers/otel.rs`: `OpenTelemetryProjection` emitting structured tracing events.
- `consumers/timeline.rs`: `TimelineProjection` & `TimelineModel` rendering millisecond-accurate ASCII and JSON execution timelines.
- `consumers/checkpoint.rs`: `CheckpointProjection` & `CheckpointEngine` with policy-driven `CheckpointPolicy` (`EveryNode`, `EveryNthNode`, `Timed`, `Manual`) and idempotent snapshotting.
- `consumers/storage.rs`: `PersistentEventStoreProjection` appending events to JSONL event logs with ordered retrieval.

---

## 4. Subsystem Governance Summary

```text
Gates (M1-M3) ──► Evidence ──► Policy Evaluation (M4) ──► Decision ──► Signed Attestation (M5)
                                                                             │
Runtime Operations ──► EventBus (ADR-017) ──► ProjectionDispatcher ──► OTel / Timeline / Checkpoint / Store
```

> **Document Revision 9 (v0.11.0 Release)**: Updated system architecture specification covering Release Governance (Epic M1–M5), Event-Driven Runtime Engine & ADR-017 ABI (v0.11 N1–N5), Projection Framework, and governance CLI tooling. Fully verified across all 67 release unit tests, 5 event unit tests, 2 attestation integration tests, 1 event integration test, and 11 CLI binary tests.
