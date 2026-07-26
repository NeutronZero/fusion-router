# FusionRouter Deployment Guide

## Prerequisites

- **Rust toolchain** (edition 2021): Install via [rustup](https://rustup.rs/)
- **System dependencies**: C compiler toolchain (MSVC Build Tools on Windows, `build-essential` on Linux, Xcode CLT on macOS)
- **SQLite**: Bundled via `rusqlite` — no system library required

## Building from Source

```bash
git clone <repo> && cd fusion-router

# Default build (includes semantic-cache via usearch)
cargo build --release

# Minimal build (no optional features)
cargo build --release --no-default-features

# With WASM plugin support
cargo build --release --features "wasm-plugins"

# With OpenTelemetry tracing
cargo build --release --features "otel"
```

The binary at `target/release/fusion-router` is self-contained with no runtime dependencies.

## Configuration

FusionRouter loads configuration from a YAML file. The config path is set via:

| Variable | Default | Description |
|---|---|---|
| `FUSION_CONFIG` | `config/default.yaml` | Path to YAML config file |

### Environment Variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `OPENCODEZEN_API_KEY` | Yes (for Zen provider) | `test-key` | API key for OpenCodeZen |
| `OPENROUTER_API_KEY` | Yes (for OpenRouter provider) | `test-key` | API key for OpenRouter |
| `OLLAMA_BASE_URL` | No (Ollama) | — | Ollama server base URL |
| `OPENCODEZEN_BASE_URL` | No | — | OpenCodeZen base URL |
| `OPENROUTER_BASE_URL` | No | — | OpenRouter base URL |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | No (otel feature) | — | OTLP gRPC endpoint |

A `.env` file in the working directory is loaded automatically at startup (via `dotenv`).

### Default Settings (from code)

| Setting | Default |
|---|---|
| Server host | `0.0.0.0` |
| Server port | `8080` |
| Shutdown timeout | 30 seconds |
| Log format | `text` (or `json`) |
| Log level | `info` |
| Rate limiting | disabled |
| CORS | Allow all origins (`*`) |
| Semantic cache | enabled (default feature) |

## Running the Server

```bash
# Minimum viable start
FUSION_CONFIG=./my-config.yaml OPENCODEZEN_API_KEY=sk-... ./fusion-router

# With OpenTelemetry
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317 \
  cargo run --release --features "otel"
```

The server listens on `0.0.0.0:8080` by default (configurable in `config.yaml`).

### Graceful Shutdown

The server handles `SIGTERM` (Unix) and `Ctrl+C` (all platforms). Outstanding requests are given `shutdown_timeout_secs` (default 30s) to complete before forced exit.

## Health Check Endpoints

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Liveness probe — always returns `{"status": "ok"}` |
| `/ready` | GET | Readiness probe — returns `{"status": "ok", "checks": {...}}` after validating database, plugins, and providers |

These can be used as container probes (see Dockerfile below) or load balancer health checks.

## Scaling

### Horizontal Scaling

FusionRouter is stateless at the HTTP layer. Scale horizontally behind a load balancer:

- Each instance maintains its own in-memory rate limiter and cache
- For shared state, use the SQLite telemetry DB on a network volume or swap for a centralized store
- No built-in clustering — coordinate via the load balancer

### Vertical Scaling

- Thread-per-core via Tokio's multi-threaded runtime (default)
- Bounded by available CPU for provider request fan-out
- Monitor memory: in-flight request state and semantic cache indices grow with concurrency

## Monitoring

### Metrics

Prometheus metrics are exposed at `/metrics` (always enabled via the `prometheus` crate — no feature flag required).

### Structured Logging

Set `logging.format: json` in config and ingest with any log shipper (Filebeat, Fluentd, etc.).

### OpenTelemetry

Build with `--features otel` to enable tracing export via OTLP. Compatible with Jaeger, Tempo, and other OTLP collectors.

### Recommended Scraping Targets

- `/metrics` — Prometheus endpoint
- `/health` — liveness check (every 10s)
- `/ready` — readiness check (every 5s)
- Application logs (JSON format) — log aggregator
