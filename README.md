# FusionRouter v0.8.0

[![Version](https://img.shields.io/badge/version-0.8.0-blue.svg)](https://github.com/NeutronZero/fusion-router/releases/tag/v0.8.0)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-251%20passed-success.svg)](https://github.com/NeutronZero/fusion-router)

An intelligent, multi-provider LLM orchestration router with DAG-based workflow planning, transactional compilation, resource budget enforcement, and semantic caching.

Supports **linear**, **conditional branching**, **loops**, **parallel split/join**, and **barrier synchronization** workflows across multiple LLM providers (OpenCode Zen, OpenRouter, Ollama).

---

## 🌟 Key Features in v0.8.0

- **7-Stage Request Pipeline**: Deterministic context assembly, intent extraction, evidence snapshotting, planning, compilation, scheduling, and provider routing.
- **Transactional Compiler**: Snapshot-and-rollback engine with 4 optimization passes (`ConstraintValidation`, `ControlFlowValidation`, `ModelResolution`, `BudgetOptimisation`) and 3-color DFS cycle detection.
- **6 Execution Strategies**: Built-in strategy sub-graph resolution for `Single`, `Consensus`, `Reflection`, `Chain`, `ReAct`, and `Debate` patterns with dynamic tool injection.
- **Resource Safety & Budgeting**: RAII `ResourceGuard` auto-refunding unused quota on error/cancellation with atomic millicost and token limits.
- **Provider Resilience**: 3-state (`Closed`, `Open`, `HalfOpen`) circuit breakers per provider with exponential backoff and prefix fallback routing.
- **Vector Semantic Caching**: USearch HNSW vector index with cosine similarity lookup and monotonic FIFO eviction.
- **WASM Plugin Sandbox**: Fuel-metered ($1\text{M}$ fuel default) Wasmtime runtime with deny-by-default WASI capabilities for custom plugins.
- **Observability & Analytics**: SQLite evidence repository in WAL mode with EMA dynamic model performance calibrator ($\alpha=0.2$, $n\ge30$), Prometheus metrics endpoint (`/metrics`), and bounded audit log ring buffer.

---

## 🚀 Quick Start

### Prerequisites
- Rust 1.75+ (2021 Edition)

### Build & Run
```bash
# Run local dev server (default port 8080)
cargo run --release

# Run comprehensive test suite (251 tests)
cargo test --all-targets

# Run performance benchmarks
cargo bench
```

---

## 🏗️ Architecture Pipeline

```
  Client Request (HTTP JSON / SSE)
               │
               ▼
   Stage 1: Context Assembly  ──────▶ Token estimation & multibyte UTF-8 trimming
               │
               ▼
   Stage 2: Requirements Extraction ─▶ Intent scoring & complexity thresholding
               │
               ▼
   Stage 3: Evidence Snapshot ─────▶ SQLite historical latency/cost lookup
               │
               ▼
   Stage 4: Workflow Planner ──────▶ Static, Dynamic, or Hybrid DAG generation
               │
               ▼
   Stage 5: Compiler Engine ───────▶ 4-Pass optimization & 3-color DFS cycle check
               │
               ▼
   Stage 6: Scheduler & Executor ──▶ Parallel WorkQueue dispatch (buffer_unordered)
               │
               ▼
   Stage 7: Provider Router ───────▶ Circuit breaker checking & LLM execution
```

---

## 📡 API Endpoints

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

## 🧪 Test Suite & Verification

FusionRouter includes a comprehensive test suite verified across 251 test cases:

```
unittests (src/lib.rs)         : 186 passed
golden_tests (tests/golden)   :  28 passed
integration_tests (tests/int) :   9 passed
load_tests (tests/load_test)  :  10 passed (8 worker threads, 100 concurrent DAGs)
security_tests (tests/sec)    :   4 passed (path traversal, shell injection, brute force)
unit_tests (tests/unit)       :  14 passed (resilience & fault injections)
-----------------------------------------------------------------------------------
Total                         : 251 passed, 0 failed
```

---

## 📚 Documentation

- [System Architecture Specification (v0.8.0)](docs/fusionrouter_architecture_v0.8.0.md)
- [Quickstart Guide](QUICKSTART.md)
- [Workflow IR Specification](docs/specifications/workflow-ir.md)
- [Execution Graph Specification](docs/specifications/execution-graph.md)
- [Architecture Decision Records (ADRs)](docs/adr/)

---

## 📄 License

Dual-licensed under MIT or Apache 2.0.
