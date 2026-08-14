# ADR-011: Testing Strategy

## Status
Accepted

## Context
FusionRouter's architecture spans async I/O, LLM provider calls, compilation pipelines, concurrent scheduling, and resource management. A robust testing strategy is needed to ensure correctness across these domains without requiring real LLM APIs. Early testing was manual and ad-hoc; a systematic approach is required for reliability and regression prevention.

## Decision

### 1. Inline Unit Tests

Tests live in `#[cfg(test)] mod tests` blocks at the bottom of each source file, co-located with the code they test. This convention ensures:
- Tests are visible during development
- Private API testing is straightforward
- Test code is compiled only during `cargo test`

### 2. Mock Providers and Embedders

Several mock implementations enable testability without real APIs:
- `MockChatProvider` — returns a fixed "mock response" for chat completion tests
- `CapturingMockProvider` — captures the `ChatCompletionRequest` for assertion
- `DummyProvider` — minimal implementation for calibration tests
- `MockEmbedder` — returns deterministic 384-dimensional vectors for semantic cache tests
- All mocks live in `#[cfg(test)]` blocks or dedicated test support code

### 3. Async Test Support

Tests use `#[tokio::test]` for async code. Axum middleware and endpoint tests bind to `127.0.0.1:0` (random port) and use `reqwest` clients to send real HTTP requests through the middleware stack.

### 4. Isolation Strategy

- SQLite-backed tests use temporary files with UUID-based names or `:memory:` databases
- Temp files are cleaned up after each test via `std::fs::remove_file`
- Plugin tests create isolated temp directories for manifest files
- No global state leaks between tests — metrics tests reset state where possible

### 5. Test Categories

Tests cover these categories by subsystem:
- **Config tests** — YAML deserialization, validation, quota/policy conversion
- **Auth tests** — disabled passthrough, valid/invalid key, whitelisted paths
- **CORS tests** — default config, empty origins, specific origin
- **Rate limiter tests** — burst allowance, block after burst, independent clients
- **Executor tests** — single/consensus strategy, system prompt injection, mock provider
- **Scheduler tests** — budget enforcement, iteration tracking, concurrent execution
- **Compiler tests** — pass execution, validation, IR transformation
- **Cache tests** — hit/miss, eviction, semantic similarity
- **Telemetry tests** — SQLite WAL mode, snapshot aggregation, cold start, model stats
- **Audit tests** — record/retrieve, max entries, JSONL export
- **Resource tests** — budget envelope, guard drop/commit, quota reservation
- **Calibration tests** — cold start skip, low success rate penalty, recovery
- **Plugin tests** — manifest discovery, malformed manifest handling

### 6. Benchmark Suite

Criterion benchmarks in `benches/`:
- `compilation` — measures compiler pass pipeline throughput
- `cache` — measures semantic cache lookup performance

Benchmarks use the `criterion` crate with HTML reports and async tokio runtime.

### 7. CI Enforcement

- `cargo check` — verifies compilation across all platforms
- `cargo test` — runs the full test suite
- `cargo test --all-features` — tests with WASM plugins, semantic cache, and OTEL enabled
- `cargo check --no-default-features --lib` — verifies bare library builds

## Consequences

- Tests are co-located with code, making them easy to find and update
- Mock providers enable deterministic testing of LLM-dependent logic
- Async tests with random ports provide realistic HTTP integration coverage
- Temp-file isolation prevents test pollution and enables parallelism
- The benchmark suite provides regression detection for performance-sensitive paths
- Feature-gated tests ensure optional dependencies don't break core builds
