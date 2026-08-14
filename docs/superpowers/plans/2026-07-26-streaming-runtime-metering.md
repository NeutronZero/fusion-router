# Sprint 1.2 — Streaming Runtime & Metering Implementation Plan

> **Goal:** Make streaming a first-class execution path with metering (token counting, latency tracking, cost calculation), cancellation (upstream propagation, resource cleanup), and telemetry recording.

**Architecture:** `MeteredStream` wraps `BoxStream<ChatStreamChunk>` with `StreamMeter` (token/latency tracking), `CancellingStream` (cancellation + resource guard), and auto-finalization on stream exhaustion.

**Tech Stack:** Rust, existing `tokio`, `futures`, `chrono`, `ResourceManager`, `FusionMetrics`, `EvidenceRepository`.

---

## File Map

```
src/resource/stream_meter.rs        # NEW — StreamMeter
src/resource/cancelling_stream.rs   # NEW — CancellingStream, MeteredStream
src/resource/mod.rs                 # MODIFY — add pub mod declarations
src/server/handlers.rs              # MODIFY — wire metering + cancellation into stream_response()
src/telemetry/stream_metrics.rs     # NEW — Streaming-specific metrics
src/telemetry/mod.rs                # MODIFY — add pub mod stream_metrics
src/types/mod.rs                    # MODIFY — add StreamCheckpoint
```

---

### Task 1: Create `StreamMeter` type

**Files:**
- Create: `src/resource/stream_meter.rs`
- Modify: `src/resource/mod.rs` (+pub mod stream_meter)

**Interfaces:**
- Consumes: `ModelPricing` from `providers`
- Produces: `StreamMeter` with token counting, timing, cost calculation

**Step 1:** Define `StreamMeter` struct

```rust
use std::time::Instant;
use crate::providers::ModelPricing;
use crate::types::Usage;

#[derive(Debug, Clone)]
pub struct StreamMeter {
    prompt_tokens: u64,
    completion_tokens: u64,
    first_chunk_at: Option<Instant>,
    last_chunk_at: Option<Instant>,
    stream_started_at: Instant,
    cost_millicosts: u64,
    finalized: bool,
}

impl StreamMeter {
    pub fn new() -> Self {
        Self {
            prompt_tokens: 0,
            completion_tokens: 0,
            first_chunk_at: None,
            last_chunk_at: None,
            stream_started_at: Instant::now(),
            cost_millicosts: 0,
            finalized: false,
        }
    }

    pub fn record_chunk(&mut self, chunk: &ChatStreamChunk, pricing: Option<&ModelPricing>) {
        let now = Instant::now();
        self.first_chunk_at.get_or_insert(now);
        self.last_chunk_at = Some(now);

        if let Some(ref usage) = chunk.usage {
            self.prompt_tokens = usage.prompt_tokens;
            self.completion_tokens = usage.completion_tokens;
        } else if let Some(ref content) = chunk.content {
            self.completion_tokens += count_tokens(content);
        }
    }

    pub fn finalize(&mut self, pricing: Option<&ModelPricing>) -> StreamMeterReport {
        if self.finalized {
            panic!("StreamMeter already finalized");
        }
        self.finalized = true;
        if let Some(p) = pricing {
            self.cost_millicosts = self.calculate_cost(p);
        }
        StreamMeterReport {
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            total_tokens: self.prompt_tokens + self.completion_tokens,
            cost_millicosts: self.cost_millicosts,
            ttfb_ms: self.first_chunk_at
                .map(|t| t.duration_since(self.stream_started_at).as_millis() as u64),
            total_duration_ms: self.last_chunk_at
                .map(|t| t.duration_since(self.stream_started_at).as_millis() as u64),
        }
    }

    fn calculate_cost(&self, pricing: &ModelPricing) -> u64 {
        let prompt_cost = (self.prompt_tokens as f64 / 1_000_000.0
            * pricing.prompt_price_per_million as f64) as u64;
        let completion_cost = (self.completion_tokens as f64 / 1_000_000.0
            * pricing.completion_price_per_million as f64) as u64;
        prompt_cost + completion_cost
    }
}

fn count_tokens(s: &str) -> u64 {
    // Rough estimate: 1 token ≈ 4 characters for English text
    (s.len() as f64 / 4.0).ceil() as u64
}
```

**Step 2:** Define `StreamMeterReport`

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamMeterReport {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cost_millicosts: u64,
    pub ttfb_ms: Option<u64>,
    pub total_duration_ms: Option<u64>,
}
```

**Step 3:** Update `src/resource/mod.rs` to add `pub mod stream_meter;`

**Step 4:** Run `cargo check` — zero new errors/warnings

**Step 5:** Commit

---

### Task 2: Create `CancellingStream` and `MeteredStream`

**Files:**
- Create: `src/resource/cancelling_stream.rs`
- Modify: `src/resource/mod.rs` (+pub mod cancelling_stream)

**Interfaces:**
- Consumes: `ResourceGuard` from `resource::guard`, `StreamMeter` from `resource::stream_meter`, `CancellationToken` from `tokio_util::sync`
- Produces: `MeteredStream` — a `Stream` implementation that wraps inner stream with metering + cancellation + guard

**Step 1:** Define `CancellingStream`

```rust
use std::pin::Pin;
use std::task::{Context, Poll};
use futures::Stream;
use pin_project::pin_project;
use tokio_util::sync::CancellationToken;

#[pin_project]
pub struct CancellingStream<S> {
    #[pin]
    inner: S,
    cancel: CancellationToken,
}

impl<S, T> Stream for CancellingStream<S>
where
    S: Stream<Item = T>,
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.cancel.is_cancelled() {
            return Poll::Ready(None);
        }
        self.project().inner.poll_next(cx)
    }
}
```

**Step 2:** Define `MeteredStream`

```rust
#[pin_project]
pub struct MeteredStream<S> {
    #[pin]
    inner: CancellingStream<S>,
    meter: Arc<Mutex<StreamMeter>>,
    guard: Option<ResourceGuard>,
    pricing: Option<ModelPricing>,
}
```

Implement `Stream` for `MeteredStream` — each `poll_next`:
1. Checks cancellation
2. Polls inner stream
3. On `Some(chunk)`: records to meter via `self.meter.lock().record_chunk(...)`
4. On `None` (stream ended): finalizes meter, releases guard
5. On drop (via `Drop` impl): releases guard

**Step 3:** Build helper function

```rust
pub fn metered_stream(
    inner: BoxStream<'static, anyhow::Result<ChatStreamChunk>>,
    guard: ResourceGuard,
    cancel: CancellationToken,
    pricing: Option<ModelPricing>,
) -> (impl Stream<Item = anyhow::Result<ChatStreamChunk>>, Arc<Mutex<StreamMeter>>) {
    let meter = Arc::new(Mutex::new(StreamMeter::new()));
    let meter_clone = meter.clone();
    let stream = MeteredStream {
        inner: CancellingStream {
            inner,
            cancel,
        },
        meter: meter_clone,
        guard: Some(guard),
        pricing,
    };
    (stream, meter)
}
```

**Step 4:** Add `tokio-util` to Cargo.toml if `CancellationToken` isn't already available (check if already a dep)

**Step 5:** Update `src/resource/mod.rs`

**Step 6:** Run `cargo check` — zero new errors/warnings

**Step 7:** Commit

---

### Task 3: Create streaming metrics

**Files:**
- Create: `src/telemetry/stream_metrics.rs`
- Modify: `src/telemetry/mod.rs` (+pub mod stream_metrics)

**Interfaces:**
- Consumes: `FusionMetrics` singleton
- Produces: Histogram/counter wrappers for streaming-specific metrics

**Step 1:** Define stream metrics

```rust
use prometheus::{register_histogram, register_counter, Histogram, Counter};

pub struct StreamMetrics {
    pub ttfb_seconds: Histogram,
    pub inter_token_latency_seconds: Histogram,
    pub streaming_duration_seconds: Histogram,
    pub streaming_tokens_total: Counter,
    pub streaming_requests_total: Counter,
    pub streaming_errors_total: Counter,
}

impl StreamMetrics {
    pub fn new() -> Self {
        Self {
            ttfb_seconds: register_histogram!(
                "fusionrouter_stream_ttfb_seconds",
                "Time to first byte for streaming responses",
                vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0]
            ).unwrap(),
            inter_token_latency_seconds: register_histogram!(
                "fusionrouter_stream_inter_token_latency_seconds",
                "Time between successive streaming chunks",
                vec![0.01, 0.05, 0.1, 0.5, 1.0]
            ).unwrap(),
            streaming_duration_seconds: register_histogram!(
                "fusionrouter_stream_duration_seconds",
                "Total duration of streaming responses",
                vec![1.0, 5.0, 10.0, 30.0, 60.0, 120.0]
            ).unwrap(),
            streaming_tokens_total: register_counter!(
                "fusionrouter_stream_tokens_total",
                "Total tokens streamed"
            ).unwrap(),
            streaming_requests_total: register_counter!(
                "fusionrouter_stream_requests_total",
                "Total streaming requests"
            ).unwrap(),
            streaming_errors_total: register_counter!(
                "fusionrouter_stream_errors_total",
                "Total streaming errors"
            ).unwrap(),
        }
    }

    pub fn record_report(&self, report: &StreamMeterReport) {
        self.streaming_tokens_total.inc_by(report.total_tokens);
        if let Some(ttfb) = report.ttfb_ms {
            self.ttfb_seconds.observe(ttfb as f64 / 1000.0);
        }
        if let Some(dur) = report.total_duration_ms {
            self.streaming_duration_seconds.observe(dur as f64 / 1000.0);
        }
    }
}

impl Default for StreamMetrics {
    fn default() -> Self { Self::new() }
}
```

**Step 2:** If `#[cfg(feature = "prometheus-metrics")]` gating is used elsewhere, gate the new metrics module the same way

**Step 3:** Update `src/telemetry/mod.rs`

**Step 4:** Run `cargo check` — zero new errors/warnings

**Step 5:** Commit

---

### Task 4: Add `StreamCheckpoint` type

**Files:**
- Modify: `src/types/mod.rs`

**Step 1:** Add the struct

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StreamCheckpoint {
    pub generation: u64,
    pub request_id: String,
    pub model: String,
    pub chunks_received: u64,
    pub content_so_far: String,
    pub completion_tokens_accumulated: u64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}
```

**Step 2:** Run `cargo check`

**Step 3:** Commit

---

### Task 5: Wire metering + cancellation into `stream_response()`

**Files:**
- Modify: `src/server/handlers.rs`
- Modify: `src/providers/mod.rs` (if CancellationToken needs threading)

**Step 1:** Update `stream_response()` in `handlers.rs`

The current `stream_response()`:
1. Gets provider from state
2. Calls `provider.chat_stream()`
3. Maps `ChatStreamChunk` → SSE `Event`
4. Returns `Sse::new()`

Updated `stream_response()`:
1. Parse model, get model requirements
2. Call `state.config_manager.snapshot()` to get current config
3. Reserve resources via `resource_manager.try_reserve(estimated_cost)`
4. Call `provider.chat_stream(request)` to get inner stream
5. Create `CancellationToken` wired to client disconnect
6. Wrap inner stream with `MeteredStream`
7. On stream completion: record telemetry via `EvidenceRepository`
8. On error: increment error metrics

**Step 2:** Handle client disconnect for upstream cancellation

Use `axum::extract::ConnectInfo` or the SSE framework's built-in disconnect detection. When the SSE response is dropped by axum (client disconnected), the `CancellationToken` fires, propagating to the upstream provider.

**Step 3:** Run `cargo check`

**Step 4:** Run `cargo test` — all existing tests pass

**Step 5:** Commit

---

### Task 6: Wire cancellation through provider chain

**Files:**
- Modify: `src/providers/router.rs`
- Modify: `src/providers/registry.rs`
- Modify: `src/providers/circuit_breaking_provider.rs`

**Step 1:** Add `CancellationToken` parameter to `chat_stream()` default impl

In `ChatProvider` trait, the default `chat_stream()` implementation just wraps `chat_completion()` in `stream::once()`. Add optional cancellation support via the CancellationToken in the provider chain. For now, the `MeteredStream` provides cancellation at the handler level — providers pass through.

**Step 2:** Thread cancellation through middleware providers

`ProviderRouter`, `ProviderRegistry`, and `CircuitBreakingProvider` all implement `chat_stream()` by delegating to inner providers. Ensure they pass along any cancellation context (the MeteredStream wraps at the handler level, so passthrough is sufficient).

**Step 3:** Run `cargo check`

**Step 4:** Commit

---

### Task 7: Unit tests

**Files:**
- Create or modify: test files

**Tests to add:**

1. `StreamMeter accumulates completion tokens` — feed chunks, verify count
2. `StreamMeter records TTFB` — first chunk sets `first_chunk_at`
3. `StreamMeter finalize records usage from final chunk` — feed final chunk with usage
4. `StreamMeter calculates cost` — verify cost from pricing + tokens
5. `CancellingStream propagates cancellation` — cancel token: stream yields None
6. `CancellingStream passes through normal items` — without cancel, items pass through
7. `MeteredStream releases guard on completion` — stream ends: guard released
8. `MeteredStream releases guard on error` — stream errors: guard released

---

### Task 8: Integration tests

**Files:**
- Modify: `tests/`

**Tests to add:**

1. `streaming request records telemetry` — full flow: metrics incremented
2. `streaming request reserves quota` — verify resource reserved
3. `concurrent streams independently metered` — N streams, verify per-stream reports
4. `client disconnect releases resources` — drop client, verify quota released
5. `streaming with bad model returns error` — invalid model → graceful error

---

### Task 9: Update `config/default.yaml` streaming section

Add streaming configuration section:

```yaml
streaming:
  enabled: true
  checkpoint_interval_chunks: 100
  default_chunk_timeout_secs: 30
```

---

## Commit Sequence

```
1. feat: add StreamMeter type with token counting and latency tracking
2. feat: add CancellingStream and MeteredStream wrappers
3. feat: add streaming-specific Prometheus metrics
4. feat: add StreamCheckpoint data type
5. feat: wire metering, cancellation, and telemetry into stream_response()
6. feat: thread cancellation through provider chain
7. test: unit tests for StreamMeter and CancellingStream
8. test: integration tests for streaming telemetry and resource lifecycle
9. docs: update config/default.yaml with streaming section
```
