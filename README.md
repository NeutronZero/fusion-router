# FusionRouter v0.10.0 Capability Platform

[![Version](https://img.shields.io/badge/version-0.10.0-blue.svg)](https://github.com/NeutronZero/fusion-router/releases/tag/v0.10.0)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-314%20passed-success.svg)](https://github.com/NeutronZero/fusion-router)
[![Architecture](https://img.shields.io/badge/architecture-v0.10.0%20frozen-purple.svg)](docs/fusionrouter_architecture_v0.10.0.md)

An **LLM orchestration operating system and capability platform** with a compiler-driven pipeline, declarative policy compilation, session continuity with triple-mode replay, and multi-channel unified ingress.

For the full design specification, see [FusionRouter v0.10.0 Architecture Specification](docs/fusionrouter_architecture_v0.10.0.md).

---

## Key Features in v0.10.0

### Compiler Pipeline (Staged Request Pipeline)
- **9-stage typed state machine**: ContextAssembly → RequirementsExtraction → EvidenceSnapshot → Planning → Compilation → ResourceReservation → SchedulingExecution → TelemetryRecording → ResponseBuilding
- **Compile-before-execute**: All `WorkflowIR` passes through a transactional compiler pipeline (constraint → control-flow → model resolution → budget) before execution
- **PrimitiveGraph IR**: Canonical lowered representation — optimization operates on the middle IR, not runtime structures

### Strategies & Execution
- **7 strategy implementations**: Single, Consensus, Reflection, Debate, ReAct, Chain, Fusion — each lowers into `PrimitiveGraph` fragments via the `Strategy` trait
- **WorkQueue scheduler**: Request-local DAG traversal via `buffer_unordered(max_concurrent)` with zero-contention state tracking
- **RAII Resource Safety**: `ResourceGuard` auto-releases quota on `Drop`; `BudgetEnvelope` enforces per-request cost/token/iteration ceilings

### Session Continuity & Replay
- **Decoupled identity**: `ExecutionSession` separated from transient `SessionSnapshot`
- **Triple replay engine**: Deterministic (exact), Inspection (side-effect free), Simulation (mock)
- **Storage-agnostic**: `SessionStore` trait with in-memory and SQLite (stub) backends; checkpoint/resume with API version validation

### Unified Ingress & Provenance
- **Canonical `ExecutionRequest`**: Single pipeline for Webhook, Cron, EventBus, and Manual triggers
- **Layered provenance chain**: TriggerTrace → PolicyTrace → ExecutionTrace

### Declarative Policy Compilation
- **AST/IR separation**: Parsed policy ASTs compile into immutable `PolicyIR`
- **Additive passes**: `PolicyCompilerPass` injects evaluation gates without modifying original graph topology
- **Precedence**: Deny > Approval > Allow

### Connector Ecosystem
- **6 reference connectors**: GitHub, Browser, MCP, Filesystem, HTTP, Shell
- **CapabilityPlugin contract**: Self-describing capability-based routing via `CapabilityContract`

### Provider Selection & Resilience
- **Capability-based model selection**: `ModelRequirements` matched against `ModelCapabilities` with cost-sorted routing
- **3-state circuit breakers**: Per-provider Closed/Open/HalfOpen with cooldown probes
- **Closed-loop calibration**: `FeedbackCalibrator` with EMA smoothing (α=0.2), cold-start guard (n≥30)

### Developer Experience & Diagnostics
- **GraphVisualizer**: Mermaid + ASCII graph output
- **TraceInspector**: Structured diagnostics viewer
- **PluginScaffolder**: Plugin project template generator
- **Prometheus metrics**: Request latency, provider latency, strategy latency, errors, graph hash distribution
- **Structured tracing**: Pipeline events with `request_id`, `strategy`, `latency_ms`, `success` fields

### Optional Feature-Gated Components
- **Semantic Vector Cache**: USearch HNSW index with cosine similarity (`semantic-cache`, default on)
- **WASM Plugin Engine**: Wasmtime 47 with fuel-metered execution (`wasm-plugins`)
- **OpenTelemetry**: OTLP gRPC exporter (`otel`)
- **Tokio Console**: Async runtime diagnostics (`dev-console`)

---

## Quick Start

### Prerequisites
- Rust 1.75+ (2021 Edition)
- API keys in `.env`:
  ```
  OPENCODEZEN_API_KEY=your_key
  OPENROUTER_API_KEY=your_key
  ```

### Build & Run
```bash
# Run local dev server (default port 8080)
cargo run

# Run all tests
cargo test

# Run performance benchmarks
cargo bench
```

### Example Request
```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "model": "auto",
    "messages": [
      {"role": "user", "content": "Write a Rust function to find prime numbers."}
    ],
    "execution": {
      "intent": "quality"
    }
  }'
```

See [QUICKSTART.md](QUICKSTART.md) for detailed setup, OpenCode integration, and configuration walkthroughs.

---

## Architecture Pipeline

```
  Client Request (HTTP JSON)
         │
         ▼
  ① Context Assembly          Token estimation & UTF-8 boundary-safe trimming
         │
         ▼
  ② Requirements Extraction   Intent scoring (keyword-based) & complexity thresholds
         │
         ▼
  ③ Evidence Snapshot         SQLite historical latency/cost lookup (FeedbackCalibrator input)
         │
         ▼
  ④ Planning                  IntentPlanner / DynamicPlanner → WorkflowIR
         │
         ▼
  ⑤ Compilation               4-pass pipeline + lowering:
         │                        ConstraintValidation → ControlFlowValidation (3-color DFS)
         │                        → ModelResolution → BudgetOptimisation → lower_to_graph()
         ▼
  ⑥ Resource Reservation      RAII ResourceGuard + per-request BudgetEnvelope
         │
         ▼
  ⑦ Scheduler & Execution     WorkQueue (buffer_unordered) → ProviderRouter → LLM API
         │
         ▼
  ⑧ Telemetry Recording       SQLite execution records + Prometheus metrics
         │
         ▼
  ⑨ Response Building         OpenAI-compatible ChatCompletionResponse assembly
```

---

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/chat/completions` | `POST` | OpenAI-compatible endpoint (standard and streaming) |
| `/health` | `GET` | Liveness probe |
| `/ready` | `GET` | Readiness probe (DB + provider checks) |
| `/metrics` | `GET` | Prometheus metrics |

---

## Test Suite & Verification

FusionRouter v0.10.0 includes **314 test cases** with 0 warnings:

```
lib / integration tests (src/, tests/)   : 207 passed
golden optimization tests                :  43 passed
integration tests                        :   9 passed
load tests (100 concurrent DAGs)         :  10 passed
security tests                           :   4 passed
strategy SDK tests                       :   7 passed
unit tests (resilience & injection)      :  34 passed
----------------------------------------------------------------
Total                                    : 314 passed, 0 failed
```

Additionally:
- `cargo check` — 0 warnings (default features)
- `cargo check --no-default-features --lib` — 0 warnings
- `cargo bench` — strategy lowering across 10 scenarios, 7 strategy types

---

## Documentation

- [System Architecture Specification (v0.10.0)](docs/fusionrouter_architecture_v0.10.0.md)
- [Architecture Decision Records (ADR-021 through ADR-031)](docs/adr/)
- [Planner Design (ADR-002)](docs/adr/ADR-002-planner.md)
- [Compiler Design (ADR-003)](docs/adr/ADR-003-compiler.md)
- [PrimitiveGraph/ExecutionGraph Alignment (ADR-019)](docs/adr/ADR-019-primitive-execution-graph-alignment.md)
- [Strategy SDK (ADR-018)](docs/adr/ADR-018-strategy-sdk.md)
- [Execution Semantics (ADR-029)](docs/adr/ADR-029-execution-semantics.md)
- [Session & Replay (ADR-030)](docs/adr/ADR-030-session-replay-semantics.md)
- [Trigger Framework (ADR-031)](docs/adr/ADR-031-trigger-request-semantics.md)
- [Quickstart Guide](QUICKSTART.md)
- [Operator Deployment Guide](docs/operator/deployment-guide.md)
- [Provenance Schema](docs/decisions/provenance-schema.md)
- [Resource Guard Contract](docs/decisions/resource-guard-contract.md)

---

## License

Dual-licensed under MIT or Apache 2.0.
