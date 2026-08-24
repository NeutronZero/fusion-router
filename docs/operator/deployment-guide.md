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

The defaults are **fail-closed** (ADR-035, v0.13.1): a default install refuses to boot unless a valid API key is configured, binds to loopback only, and has tool execution disabled.

| Setting | Default |
|---|---|
| Server host | `127.0.0.1` |
| Server port | `8080` |
| Shutdown timeout | 30 seconds |
| Request timeout | 300 seconds (`server.request_timeout_secs`) — also caps runaway streaming responses |
| Concurrency envelope | `resources.max_concurrent` in-flight requests server-wide (503 beyond) |
| Log format | `text` (or `json`) |
| Log level | `info` |
| Authentication | enabled (requires at least one `api_keys` entry) |
| Rate limiting | enabled (60 req/min, burst 10, keyed on peer address or authenticated identity) |
| CORS | same-origin only (empty `allowed_origins`) |
| Shell commands | none (`allowed_shell_commands: []`) |
| HTTP tool | disabled (`enable_http_tool: false`) |
| Semantic cache | enabled (default feature) |

In release builds a provider whose API key cannot be resolved **refuses to boot**; the empty-key
startup path exists only under `--unsafe-dev`.

### `--unsafe-dev` (development only)

Release builds reject insecure combinations at startup: auth disabled, rate limiting disabled, wildcard CORS (`*`), non-empty shell allowlist, or the HTTP tool enabled — each fails `validate()` unless the server is started with `--unsafe-dev` (which logs a prominent warning and is never appropriate for a network-exposed deployment).

## API keys & scopes

Every entry in `auth.api_keys` is valid for the chat surface
(`/v1/chat/completions`, `/v1/messages`). Append `:operator` to grant the
**operator scope**, additionally required for:

- `/v1/operations/*` (policies, dashboard, inspector, attestations)
- `/v1/executions`
- `/metrics`

```yaml
auth:
  enabled: true
  api_keys:
    - "sk-chat-client"          # chat only
    - "sk-ops-team:operator"    # chat + operator surfaces
```

Keys are compared in constant time; entries with unknown scope suffixes are rejected at load.

## Live reload (SIGHUP)

Sending SIGHUP hot-applies — no restart:

- **API keys & scopes** (rotated-out keys are rejected on the next request)
- **Rate-limit settings** (rpm/burst; existing buckets keep their state)
- Provider registry and connector set

Tool allowlists and scheduler concurrency still require a restart. The reload log lists what was applied.

Tool allowlists and scheduler concurrency still require a restart. The reload log lists what was applied.

## Running the Server

```bash
# Minimum viable start (fail-closed: set an API key in config first)
FUSION_CONFIG=./my-config.yaml OPENCODEZEN_API_KEY=sk-... ./fusion-router

# Local development without auth/rate limiting (do not expose publicly)
./fusion-router --unsafe-dev

# With OpenTelemetry
OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317 \
  cargo run --release --features "otel"
```

The server listens on `127.0.0.1:8080` by default (configurable in `config.yaml`).

### Graceful Shutdown

The server handles `SIGTERM` (Unix) and `Ctrl+C` (all platforms). Outstanding requests are given `shutdown_timeout_secs` (default 30s) to complete before forced exit.

## Health Check Endpoints

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Liveness probe — always returns `{"status": "ok"}` |
| `/ready` | GET | Readiness probe — pings the telemetry database and verifies providers are configured; returns 503 with per-check status when unhealthy |

`/metrics` requires an **operator-scoped key** (see API keys & scopes above).

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
