# FusionRouter v0.14 LTS Foundation — Compiler-Driven AI Orchestration Platform

[![Version](https://img.shields.io/badge/version-0.14.0--LTS-blue.svg)](https://github.com/NeutronZero/fusion-router/releases/tag/v0.14.0)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Architecture](https://img.shields.io/badge/architecture-AF--005%20frozen-purple.svg)](docs/developer/handbook.md)
[![Status](https://img.shields.io/badge/status-LTS%20Foundation%20Certified-success.svg)](docs/governance/v1-readiness-report.md)

> **"FusionRouter is a compiler-driven AI orchestration platform that converts user intent into deterministic execution graphs, providing explainable routing, operational governance, portable execution bundles, and deterministic replay."**

```text
Compile AI Workflows.
Execute Anywhere.
Explain Every Decision.
Replay Every Execution.
```

---

## Key Features in v0.14 LTS Foundation

### 1. Governed Platform Architecture (AF-003, AF-004, AF-005 Freezes)
- **AF-003 Architecture Freeze:** 17 Architecture Laws, 11 Architectural Invariants, strict 3-tier Cargo workspace hierarchy (`Foundation -> Engine -> Platform -> Applications`).
- **AF-004 Platform Contract Freeze (`v1`):** Versioned, frozen schemas for `WorkflowIR v1`, `Execution ABI v1`, `REST API v1`, `Worker Protocol v1`, `Plugin SDK v1`, `ExecutionBundle v1`, `Compiler Report v1`, `Dashboard API v1`, `Health Report v1`.
- **AF-005 Repository Layout Freeze:** Locked workspace topology (`crates/`, `apps/`, `ui/`, `docs/`, `tests/`) protecting plugin authors and platform consumers against structural drift.

### 2. Compiler Primacy & Zero Bypass Governance
- Every request traverses the `Planner -> Compiler -> Scheduler -> Runtime` pipeline. Zero bypass paths.
- **9-Pass Optimization Pipeline:** `Validation`, `CapabilityResolution`, `ConstraintSolver`, `ConstantFolding`, `DeadNodeElimination`, `NodeFusion`, `RetryInjection`, `FallbackInjection`, `SchedulingHints`.
- **Fine-Grained Capability Catalog (`CapabilityRegistry`):** Reasoning over specific capabilities (`Vision`, `JSON`, `ToolCalling`, `Reasoning`, `Streaming`, `Embeddings`, `Audio`, `ImageGen`, `Video`, `MCP`).
- **Intent Execution Profiles (`ExecutionProfile`):** Lowering user intent (`Fast`, `Balanced`, `Cheap`, `Coding`, `Research`, `Vision`, `Reasoning`, `Creative`, `Offline`) into compiler policies.

### 3. Portable Execution Intelligence & Deterministic Replay Engine
- **Portable Execution Bundles (`.fusion` Export/Import):** Complete trace snapshots containing `ExecutionRecord`, `WorkflowIR`, `CompilerReport`, `Timeline`, `Telemetry`, and `ConfigSnapshot`.
- **3-Mode Deterministic Replay:** Step through executions via `Timeline Replay`, `Compiler Pass Replay` (with time-travel pass diffing), or `Runtime Replay`.
- **100% Replay Fidelity Guarantee:** Verified by `tests/beta_replay.rs`.

### 4. Mission Control & Operational Health Management
- **Fusion Studio Web Dashboard (`http://localhost:8080`):** Live Mission Control overview, 5-Tab Compiler Inspector, Provider Lifecycle Manager (zero-restart live hot-reload), and System Diagnostics.
- **Platform Health Engine:** 9-domain health checklist (`API Gateway`, `SQLite Database`, `Local Ollama Probe`, `Provider Connectivity`) with automated diagnostic recovery actions.
- **Certified Performance SLOs (Invariant 11):** Enforced in CI via [`docs/slo/manifest.yaml`](docs/slo/manifest.yaml) (Planner `<10ms`, Compiler `<20ms`, Scheduler `<5ms`, Runtime Overhead `<10ms`, Replay `<20ms`).

---

## 3-Tier Workspace Architecture

```text
fusion-router/
├── crates/
│   ├── Tier 1: Foundation (fusion-core, fusion-kernel, fusion-api-internal)
│   ├── Tier 2: Domain Engine (fusion-planner, fusion-compiler, fusion-scheduler, fusion-runtime)
│   └── Tier 3: Platform (fusion-infrastructure, fusion-security, fusion-api-public, fusion-studio-api, fusion-plugin-sdk, fusion-worker-protocol, fusion-worker)
├── apps/
│   └── fusion-server/             (Executable binary entry point & Bootstrap sequence)
├── ui/                            (Studio React + TS Frontend, Design Tokens & UI Extension SDK)
└── docs/                          (User Guide, Operator Guide, Architecture Handbook, Tutorials, Cookbook)
```

---

## Quick Start

### Prerequisites
- Rust 1.75+ (2021 Edition)

### Build & Run
```bash
# Run local FusionRouter Studio server (http://localhost:8080)
cargo run -p fusion-server

# Run full workspace test suite
cargo test --workspace

# Run Architecture Conformance & Contract Compatibility tests
cargo test --test conformance --test compatibility_v1 --test performance_slo
```

---

## Documentation Platform

- [v1.0 Release Readiness & Certification Report](docs/governance/v1-readiness-report.md)
- [Architecture Handbook (AF-003 / AF-004 / AF-005)](docs/developer/handbook.md)
- [User Guide & Onboarding](docs/user/index.md)
- [Operator Deployment & Health Engine Guide](docs/operator/index.md)
- [Step-by-Step Guided Tutorials (1–8)](docs/tutorials/index.md)
- [Practical Routing Cookbook](docs/cookbook/index.md)
- [Performance SLO Manifest](docs/slo/manifest.yaml)

---

## License

Dual-licensed under MIT or Apache 2.0.
