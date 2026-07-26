# Sprint 1.2 — Streaming Runtime & Metering Design

> **Theme:** Making streaming a first-class execution path with metering, cancellation, and replay.
> **Status:** Draft Design
> **Predecessor:** Sprint 1.1 — Live Configuration (Epic G)

---

## 1. Problem Statement

Streaming currently bypasses every operational layer:

| Concern | Non-streaming | Streaming |
|---------|---------------|-----------|
| Resource reservation | `ResourceManager::try_reserve()` | None |
| Budget enforcement | `BudgetEnvelope` check per step | None |
| Token metering | `FusionMetrics::tokens_total` | None |
| Cost tracking | Accumulated per-node | None |
| Latency tracking | Provider latency recorded | None |
| Telemetry recording | `EvidenceRepository::record()` | None |
| Cancellation | CancellationToken + Scheduler | Axum drops stream (no upstream propagation) |
| Replay | Snapshot-based | Not supported |
| Strategy execution | Full pipeline | Bypassed entirely |

This means every streaming request is invisible to operations, unrecoverable, and uncapped.

---

## 2. Architecture

### 2.1 Stream Wrapper Pattern

The core design is a **`StreamWrapper`** that decorates a `BoxStream` with metering, cancellation, and lifecycle hooks:

```
Provider::chat_stream()
  -> StreamWrapper::new(inner_stream, meter, cancel, guard)
    -> yield metered chunks
    -> on completion: finalize meter, release guard
    -> on cancellation: propagate upstream, release guard
    -> SSE response
```

Each `StreamWrapper` owns:
- A **`StreamMeter`** that accumulates tokens and tracks timing
- An **optional `ResourceGuard`** that is released on stream exhaustion or drop
- A **`CancellationToken`** that triggers upstream cancellation on drop

### 2.2 Request Lifecycle

```
1. Handler receives streaming request (stream: true)
2. Pipeline branches to stream_response() vs process_request()
3. stream_response():
   a. Parse request, extract model requirements
   b. Reserve resources via ResourceManager::try_reserve()
   c. Select provider via ProviderRegistry
   d. Call provider.chat_stream() to get inner stream
   e. Wrap inner stream in MeteredStream with:
      - StreamMeter (token counter + latency tracker)
      - ResourceGuard (linked to reservation)
      - CancellationToken (wired to client disconnect)
   f. Map metered chunks to SSE events
   g. On completion: record telemetry, finalize meter
   h. On error/cancel: record error telemetry, release resources
```

### 2.3 Stream Ownership

```
Request ──→ Sse::new(stream) ──→ axum
              │
              ▼
         MeteredStream
              │
              ├── StreamMeter (shared: Arc<Mutex<StreamMeter>>)
              ├── ResourceGuard (RAII, released on drop)
              └── CancellationToken (drop = cancel)
```

Ownership chain:
- AXUM owns the SSE response stream
- `MeteredStream` wraps the inner `BoxStream<ChatStreamChunk>`
- `ResourceGuard` is held by the `MeteredStream` — dropped when the stream is exhausted OR when the client disconnects (axum drops the response)
- `StreamMeter` is shared via `Arc` so telemetry can read it after the stream completes

---

## 3. Metering Model

### 3.1 `StreamMeter`

```rust
pub struct StreamMeter {
    // Accumulated during streaming
    completion_tokens: u64,
    // Known at stream start (from final chunk or request)
    prompt_tokens: u64,
    // Timing
    first_chunk_at: Option<Instant>,
    last_chunk_at: Option<Instant>,
    stream_started_at: Instant,
    // Cost (derived from model pricing)
    cost_millicosts: u64,
}
```

### 3.2 Token Accumulation

The `ChatStreamChunk` has optional `usage` which appears only in the **final chunk** from most providers:

```json
// First N chunks:
{"choices":[{"delta":{"content":"Hello"}}]}

// Final chunk:
{"choices":[{"delta":{}}],"usage":{"prompt_tokens":50,"completion_tokens":150,"total_tokens":200}}
```

Strategy:
1. Track **completion_tokens** by counting characters/words during streaming (optional — not all providers send per-chunk token counts)
2. Extract **prompt_tokens** and **completion_tokens** from the final chunk's `usage` field if present
3. Fall back to `StreamMeter::estimate_cost()` using model pricing when usage is absent

### 3.3 Latency Tracking

| Metric | Definition | Source |
|--------|------------|--------|
| TTFB | Time from request received to first chunk yielded | `StreamMeter` |
| Inter-token latency | Time between successive chunks | `StreamMeter` |
| Total latency | Time from request received to final chunk | `StreamMeter` |

### 3.4 Cost Calculation

```rust
fn calculate_cost(&self, pricing: &ModelPricing) -> u64 {
    let prompt_cost = (self.prompt_tokens as f64 / 1_000_000.0 * pricing.prompt_price_per_million as f64) as u64;
    let completion_cost = (self.completion_tokens as f64 / 1_000_000.0 * pricing.completion_price_per_million as f64) as u64;
    prompt_cost + completion_cost
}
```

---

## 4. Cancellation Semantics

### 4.1 Downstream Disconnect → Upstream Cancellation

```
Client disconnects
  → Axum drops Sse stream
    → MeteredStream is dropped
      → ResourceGuard::drop() releases quota
      → CancellationToken::cancel() signals upstream
        → Provider::chat_stream() receives cancel
          → Underlying HTTP request is aborted
```

### 4.2 Implementation

```rust
pub struct CancellingStream {
    inner: BoxStream<'static, anyhow::Result<ChatStreamChunk>>,
    cancel: CancellationToken,
    guard: Option<ResourceGuard>,
}

impl Stream for CancellingStream {
    type Item = anyhow::Result<ChatStreamChunk>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.cancel.is_cancelled() {
            return Poll::Ready(None);
        }
        self.inner.poll_next_unpin(cx)
    }
}
```

When the stream wrapper is dropped (client disconnect):
1. `drop()` is called (Rust guarantees this for owned streams)
2. `ResourceGuard::drop()` releases the reserved quota
3. `CancellationToken::cancel()` signals the provider to abort the HTTP request

### 4.3 Upstream Abort

For HTTP transports, cancellation means dropping the response stream. The `reqwest` `Response::bytes_stream()` is tied to the response — when the stream is dropped, the TCP connection is closed.

For providers: the `CancellationToken` is wired to the `Provider::chat_stream()` call — but since the `MeteredStream` wraps the outer stream, dropping it will cascade through to the underlying transport.

---

## 5. Replay & Checkpoints

### 5.1 Stream Checkpoint Format

```rust
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

Checkpoints are saved periodically (every N chunks or every M bytes) during streaming.

### 5.2 Partial Replay

A partial replay replays only the un-consumed portion:

1. Load checkpoint
2. Re-construct `ChatCompletionRequest` (same model, messages)
3. Append the content-so-far as an assistant message
4. Truncate the original completion by `content_so_far` length
5. Stream the remaining content

This is **Sprint 1.2+** work — the initial design includes the data structures but the full partial replay implementation is deferred.

### 5.3 Deterministic Boundaries

Stream replay is non-deterministic by nature — providers may produce different content for the same prompt at different times. The replay guarantee is:

**Structural replay:** The number of chunks, their timing metadata, and the final aggregated content are preserved. The exact content of each chunk is NOT guaranteed to match — only the full concatenation.

This is defined but not fully implemented in Sprint 1.2. The initial sprint focuses on the metering and cancellation infrastructure.

---

## 6. File Map

```
src/resource/stream_meter.rs        # NEW: StreamMeter (token counting, latency tracking)
src/resource/cancelling_stream.rs   # NEW: CancellingStream (wrapper with cancellation)
src/resource/mod.rs                 # MODIFY: +pub mod stream_meter; +pub mod cancelling_stream;
src/server/handlers.rs              # MODIFY: Wire metering + cancellation into stream_response()
src/providers/mod.rs                # MODIFY: Add CancellationToken to ChatProvider::chat_stream() default
src/telemetry/stream_metrics.rs     # NEW: Streaming-specific metrics (TTFB, inter-token latency)
src/telemetry/mod.rs                # MODIFY: +pub mod stream_metrics;
src/types/mod.rs                    # MODIFY: +StreamChunkMetadata, +StreamCheckpoint
src/providers/router.rs             # MODIFY: Pass cancellation through
src/providers/registry.rs           # MODIFY: Pass cancellation through
src/providers/circuit_breaking_provider.rs  # MODIFY: Pass cancellation through
```

---

## 7. Testing Strategy

### 7.1 Unit Tests

| Test | What |
|------|------|
| `StreamMeter accumulates completion tokens` | Feed chunks, verify running total |
| `StreamMeter records TTFB` | Verify first_chunk_at is set on first poll |
| `StreamMeter calculates cost` | Feed chunks + pricing, verify cost output |
| `StreamMeter finalize records usage` | Feed final chunk with usage, verify totals |
| `CancellingStream propagates cancel` | Cancel token, verify stream yields None |
| `CancellingStream releases guard on drop` | Drop stream, verify ResourceGuard released |
| `CancellingStream passes through normal items` | Normal stream, verify items pass through |

### 7.2 Integration Tests

| Test | What |
|------|------|
| `streaming request records telemetry` | Full request → verify metrics incremented |
| `streaming request reserves quota` | Verify resource reservation before stream |
| `client disconnect releases quota` | Drop client, verify quota released |
| `streaming with invalid model returns error` | Bad model → check error path |
| `concurrent streams metered independently` | N concurrent streams, verify telemetry per-stream |

### 7.3 Existing Tests That Must Remain Green

All existing tests (534 passing) must remain green after Sprint 1.2 implementation.

---

## 8. Constraints

- **No breaking changes** to the `ChatProvider` trait signature if possible
- **No new dependencies** (use existing tokio, futures, chrono)
- **Cancellation must not leak resources** under any failure path
- Backward compatibility with existing non-streaming pipeline
- All streaming-related types should be behind `#[cfg(feature = "streaming-runtime")]` or simply added as non-gated (since streaming is a core path)
