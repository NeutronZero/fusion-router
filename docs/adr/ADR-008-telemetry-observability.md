# ADR-008: Telemetry & Observability

## Status
Accepted

## Context
FusionRouter needs observability across all pipeline stages — request tracing, performance metrics, cost tracking, and evidence-based model calibration. Early versions had no structured telemetry, making it impossible to debug production issues or optimize provider selection. Requirements include: distributed tracing, Prometheus metrics, execution audit trail, and cost/latency aggregation for the feedback calibration loop.

## Decision

### 1. Structured Logging with `tracing`

All logging uses the `tracing` crate with `tracing-subscriber` for output:
- Text format for development (default)
- JSON format for production (`logging.format: "json"`)
- Level filtering via `EnvFilter` from `RUST_LOG` or config
- Feature-gated `console-subscriber` for tokio console (`dev-console` feature)

### 2. OpenTelemetry Tracing (Feature-Gated)

The `otel` feature enables OTLP export via `opentelemetry-otlp`:
- Spans exported to a configurable OTLP endpoint (`OTEL_EXPORTER_OTLP_ENDPOINT`, default `http://localhost:4317`)
- `service.name` set to `"fusion-router"`
- Batch export with tokio runtime
- Feature-gated to avoid pulling in tonic/opentelemetry dependencies by default

### 3. Prometheus Metrics

A `FusionMetrics` singleton (via `OnceLock`) exposes:
- `fusionrouter_requests_total` — request counter
- `fusionrouter_request_duration_seconds` — histogram labelled by route
- `fusionrouter_errors_total` — error counter
- `fusionrouter_tokens_total` — token consumption counter
- `fusionrouter_provider_latency_seconds` — histogram labelled by provider

Metrics are rendered via the `/metrics` endpoint using `prometheus::TextEncoder`.

### 4. Audit Log

An in-memory `AuditLog` records structured `AuditEntry` items (timestamp, request_id, user_id, action, result, details). Configurable max capacity (default 1000). Supports JSONL serialization for external consumption.

### 5. SQLite Evidence Repository

An `EvidenceRepository` trait backed by SQLite stores per-execution records:
- `execution_records` table with WAL mode, busy timeout, and indexed columns
- Records contain: model, provider, intent, latency, tokens, cost, success status
- Aggregation queries for snapshots (success rates, avg latencies, avg costs, model rankings)
- Model performance stats with configurable time window
- Records are inserted asynchronously via `spawn_blocking`

### 6. Continuous Feedback Calibration

A `FeedbackCalibrator` periodically reads execution evidence, computes health factors from success rates, and updates provider model capabilities in the `ProviderRegistry`:
- Configurable sample size threshold (default 30), smoothing factor, time window
- Calibration loop runs on a configurable interval with cancellation token
- Degraded models have their coding/reasoning scores reduced, affecting provider selection

## Consequences

- Operators get Prometheus metrics, structured logs, and distributed tracing out of the box
- The calibration loop enables self-healing provider selection based on real performance
- Audit log provides a lightweight compliance trail without external dependencies
- SQLite telemetry is self-contained — no external database required
- Feature flags keep heavy dependencies (OTLP, console) optional
- WAL mode on SQLite ensures read/write concurrency in async contexts
