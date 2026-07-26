# FusionRouter v0.9.0

[![Version](https://img.shields.io/badge/version-0.9.0-blue.svg)](https://github.com/NeutronZero/fusion-router/releases/tag/v0.9.0)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-444%20passed-success.svg)](https://github.com/NeutronZero/fusion-router)

An intelligent, multi-provider LLM orchestration router with compiler-driven workflow planning, optimization passes, DAG execution, and provenance-based execution replay.

Supports **single-shot**, **consensus**, **reflection**, **debate**, **ReAct tool loops**, **chain pipelines**, and **fusion** strategies across multiple LLM providers (OpenRouter, OpenCode Zen, Ollama).

---

## Key Features in v0.9.0

### Compiler Architecture
- **8-Stage Compiler Pipeline**: Validation → Resolution → Lowering → Optimization → Assembly, with snapshot-and-rollback transactional semantics.
- **Canonical `PrimitiveGraph` IR**: Lowered intermediate representation on which all optimization passes operate; `ExecutionGraph` is a mechanically derived ephemeral artifact.
- **`StrategyIR` Lowering Contract**: Each strategy receives a `StrategyIR` enum and produces a `PrimitiveGraph` fragment via `Strategy::lower()`, decoupling strategy logic from execution metadata.
- **Optimization Framework** (ADR-020): Pass taxonomy (Validation/Lowering/Analysis/Optimization/Instrumentation/Verification), optimization goals, legality rules, pre/post-condition contracts, and rollback-safe pipeline.
- **Dead Node Elimination**: Removes nodes unreachable from the first node.
- **FanOut Consolidation**: Merges adjacent FanOut nodes and eliminates single-consumer FanOuts.

### Execution Strategies
- **7 Built-in Strategies**: `Single`, `Consensus`, `Reflection`, `Chain`, `ReAct`, `Debate`, and `Fusion` (new) — all producing `PrimitiveGraph` via `Strategy::lower()`.
- **`FusionStrategy`**: Heterogeneous model ensembles with `ModelAvailability`/`ModelCapability` hints and parallelism scaling.

### Determinism & Provenance
- **Deterministic Graph Hashing**: `PrimitiveGraph::compute_hash()` via canonical JSON serialization; `ExecutionGraph` node IDs derived from `(graph_hash, node_index)`.
- **Provenance Schema**: Every `ExecutionResult` carries `graph_hash`, `primitive_graph_version`, `pass_manifest`, and `strategy` descriptor for full execution replay and audit.
- **`Artifact` Trait**: Typed opaque payloads stored on `ExecutionResult` with forward-compatible clone semantics.

### Observability
- **Per-Strategy Metrics**: `fusionrouter_strategy_latency_seconds` and `_errors_total` histograms with strategy labels; `fusionrouter_graph_hash_count` provenance distribution.
- **Structured Tracing**: Pipeline events with `request_id`, `strategy`, `latency_ms`, and `success` fields.
- **Operator Deployment**: Docker multi-stage build (distroless base, healthcheck), deployment guide, and K8s probe contract.

### Resource Safety
- **ResourceGuard**: RAII auto-refunding unused quota on Drop if uncommitted.
- **BudgetEnvelope**: Per-request cost/token/iteration ceilings via `Arc<AtomicU64>`.

### Provider Resilience
- **3-State Circuit Breakers** (Closed/Open/HalfOpen) per provider with exponential backoff and prefix fallback routing.
- **Closed-Loop Calibration**: EMA smoothing ($\alpha=0.2$, $n\ge30$) of provider capability scores.

---

## Quick Start

### Prerequisites
- Rust 1.75+ (2021 Edition)

### Build & Run
```bash
# Run local dev server (default port 8080)
cargo run --release

# Run comprehensive test suite (444 tests)
cargo test --all-targets

# Run performance benchmarks
cargo bench
```

---

## Architecture Pipeline

```
  Client Request (HTTP JSON / SSE)
               │
               ▼
   Stage 1: Context Assembly          Token estimation & multibyte UTF-8 trimming
               │
               ▼
   Stage 2: Requirements Extraction   Intent scoring & complexity thresholding
               │
               ▼
   Stage 3: Evidence Snapshot         SQLite historical latency/cost lookup
               │
               ▼
   Stage 4: Workflow Planner          Static, Dynamic, or Hybrid → WorkflowIR
               │
               ▼
   Stage 5: Compiler Engine           8-stage pipeline:
               │                        1. ConstraintValidation
               │                        2. ControlFlowValidation (3-color DFS)
               │                        3. ModelResolution
               │                        4. BudgetOptimisation
               │                        5. LowerToGraph (WorkflowIR → PrimitiveGraph)
               │                        6. DeadNodeElimination
               │                        7. FanOutConsolidation
               │                        8. PrimitiveToExecution (PrimitiveGraph → ExecutionGraph)
               ▼
   Stage 6: Scheduler & Executor      Parallel WorkQueue dispatch (buffer_unordered)
               │
               ▼
   Stage 7: Provider Router           Circuit breaker checking & LLM execution
```

---

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/chat/completions` | `POST` | OpenAI-compatible endpoint supporting standard and streaming (`stream: true`) requests |
| `/health` | `GET` | Liveness probe returning `200 OK` |
| `/ready` | `GET` | Readiness probe verifying provider connectivity |
| `/metrics` | `GET` | Prometheus telemetry metrics |

### Example Request

```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "model": "auto",
    "messages": [
      {"role": "user", "content": "Write a rust function to find prime numbers."}
    ],
    "execution": {
      "intent": "Quality"
    }
  }'
```

---

## Test Suite & Verification

FusionRouter v0.9.0 includes a comprehensive test suite verified across **444 test cases** with 0 warnings:

```
lib tests (src/lib.rs)               : 177 passed
binary tests (src/main.rs)           : 177 passed
golden optimization tests            :  43 passed
integration tests                    :   9 passed
load tests (100 concurrent DAGs)     :  10 passed
security tests                       :   4 passed
strategy SDK tests                   :   7 passed
unit tests (resilience & injection)  :  17 passed
----------------------------------------------------------------
Total                                : 444 passed, 0 failed
```

Additionally:
- `cargo check` — 0 warnings (default features)
- `cargo check --no-default-features --lib` — 0 warnings
- `cargo bench` — strategy lowering benchmarks across 10 scenarios, 7 strategy types

---

## Documentation

- [System Architecture Specification (v0.9.0)](docs/fusionrouter_architecture_v0.9.0.md)
- [Architecture Decision Records (ADRs)](docs/adr/)
- [Provenance Schema](docs/decisions/provenance-schema.md)
- [Resource Guard Contract](docs/decisions/resource-guard-contract.md)
- [Operator Deployment Guide](docs/operator/deployment-guide.md)
- [Quickstart Guide](QUICKSTART.md)

---

## License

Dual-licensed under MIT or Apache 2.0.
