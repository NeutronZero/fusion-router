# FusionRouter Telemetry & Observability

## Overview

FusionRouter provides comprehensive observability through events, metrics, tracing, and audit logging. The system is designed so telemetry never blocks request processing.

**Location:** `src/telemetry/`, `src/events/`

## Event System

**Location:** `src/events/`

The Runtime Event Stream is the observability ABI — an event sourcing substrate for all runtime observability.

| Component | File | Purpose |
|-----------|------|---------|
| `ExecutionEventEnvelope` | `src/events/payload.rs` | Immutable event envelope with monotonic sequence |
| `EventBus` trait | `src/events/mod.rs` | Core event bus interface |
| `BroadcastEventBus` | `src/events/bus.rs` | In-memory broadcast implementation |
| `ProjectionDispatcher` | `src/events/projection.rs` | Decoupled event production → consumption |

### Event Projections

| Consumer | File | Purpose |
|----------|------|---------|
| Timeline | `src/events/consumers/timeline.rs` | Execution timeline for visualization |
| Storage | `src/events/consumers/storage.rs` | Persistent event storage |
| OTel | `src/events/consumers/otel.rs` | OpenTelemetry export (feature-gated) |
| Checkpoint | `src/events/consumers/checkpoint.rs` | Checkpoint-triggered snapshots |

## Evidence Repository (`src/telemetry/`)

| Component | File | Purpose |
|-----------|------|---------|
| `EvidenceRepository` trait | `src/telemetry/mod.rs` | Evidence storage interface |
| `SqliteEvidenceRepository` | `src/telemetry/sqlite_repo.rs` | SQLite-backed evidence storage (WAL mode, bundled rusqlite) |
| `FeedbackCalibrator` | `src/telemetry/calibration.rs` | Closed-loop feedback calibration (EMA α=0.2, cold-start n≥30) |

## Metrics (`src/telemetry/metrics.rs`)

| System | Feature Gate | Description |
|--------|--------------|-------------|
| Prometheus | `prometheus-metrics` | Prometheus metric collection |
| `FusionMetrics` | Always | Core metrics (request count, latency, error rates) |
| `StreamMetrics` | `src/telemetry/stream_metrics.rs` | Streaming execution metrics |
| `ConnectorMetrics` | `src/telemetry/connector_metrics.rs` | Connector-specific metrics |

## Tracing (`src/telemetry/tracing.rs`)

| Feature | Gate | Backend |
|---------|------|---------|
| Console | `dev-console` | `console-subscriber` (tokio console) |
| OTLP | `otel` | OpenTelemetry OTLP exporter |

### Tracing Configuration

- Environment filter (`RUST_LOG`)
- JSON or text format
- Registry layer for composed subscribers

## Audit Log (`src/telemetry/audit.rs`)

Structured audit logging for security-relevant events:
- API access
- Configuration changes
- Policy evaluations
- Release governance actions

## Unified Diagnostics (`src/telemetry/unified_diagnostics.rs`)

Combines tracing events, metrics, and audit entries into unified diagnostic output.

## Developer Experience (`src/devex/`)

| Tool | File | Purpose |
|------|------|---------|
| `GraphVisualizer` | `src/devex/visualizer.rs` | Visualizes `ExecutionGraph` structure |
| `TraceInspector` | `src/devex/trace_inspector.rs` | Execution trace inspection |
| `PluginScaffolder` | `src/devex/scaffold.rs` | Plugin project scaffolding |
| Timeline commands | `src/devex/commands/` | `fusion trace timeline/events` |

## Key Invariants

- Evidence is written after execution, never during
- Telemetry never blocks request processing
- Events are immutable after emission
- Event bus decouples producers from consumers via ProjectionDispatcher
- Feedback calibration uses exponential moving average

## Related ADRs

- ADR-008: Telemetry & Observability (tracing, OTel, Prometheus, evidence)
- ADR-017 (docs/adrs/): Runtime Event Stream ABI
