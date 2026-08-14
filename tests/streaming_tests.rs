use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures::{Stream, StreamExt, stream};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use fusion_router::resource::cancelling_stream::metered_stream;
use fusion_router::resource::stream_meter::StreamMeterReport;
use fusion_router::resource::{DefaultResourceManager, ResourceGuard, ResourceManager};
use fusion_router::telemetry::stream_metrics::StreamMetrics;
use fusion_router::types::{
    ChatStreamChunk, ExecutionGraph, GraphMetadata, Quota, StreamCheckpoint, Usage,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_graph() -> ExecutionGraph {
    ExecutionGraph {
        graph_id: Uuid::new_v4(),
        nodes: vec![],
        edges: vec![],
        metadata: GraphMetadata {
            estimated_cost: fusion_router::types::NanoUSD::ZERO,
            estimated_tokens: 100,
            max_depth: 1,
            node_count: 0,
        },
        total_tokens: 100,
        total_cost: fusion_router::types::NanoUSD::ZERO,
        primitive_graph_hash: 0,
    }
}

fn test_quota() -> Quota {
    Quota {
        max_daily_cost: fusion_router::types::NanoUSD::from_nanos(1_000_000_000_000),
        max_daily_tokens: 1_000_000_000,
        max_concurrent: 1000,
        provider_limits: HashMap::new(),
    }
}

fn make_chunks() -> Vec<anyhow::Result<ChatStreamChunk>> {
    vec![
        Ok(ChatStreamChunk {
            content: Some("Hello ".to_string()),
            finish_reason: None,
            usage: None,
        }),
        Ok(ChatStreamChunk {
            content: Some("world".to_string()),
            finish_reason: None,
            usage: None,
        }),
        Ok(ChatStreamChunk {
            content: None,
            finish_reason: Some("stop".to_string()),
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        }),
    ]
}

/// Stream that yields N chunks with a `delay` between each.
fn slow_chunks(
    count: usize,
    delay: Duration,
) -> impl Stream<Item = anyhow::Result<ChatStreamChunk>> {
    futures::stream::unfold(0usize, move |i| {
        let d = delay;
        async move {
            if i >= count {
                return None;
            }
            tokio::time::sleep(d).await;
            let chunk = Ok(ChatStreamChunk {
                content: Some(format!("chunk {i}")),
                finish_reason: None,
                usage: None,
            });
            Some((chunk, i + 1))
        }
    })
}

fn make_resource_guard(manager: Arc<dyn ResourceManager>) -> ResourceGuard {
    ResourceGuard::new(Uuid::new_v4(), test_graph(), manager)
}

// ---------------------------------------------------------------------------
// Test 1 — MeteredStream counts tokens correctly
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_metered_stream_counts_tokens() {
    let chunks = make_chunks();
    let inner: Pin<Box<dyn Stream<Item = anyhow::Result<ChatStreamChunk>> + Send>> =
        Box::pin(stream::iter(chunks));

    let manager: Arc<dyn ResourceManager> =
        Arc::new(DefaultResourceManager::new(test_quota()));
    manager.try_reserve(&test_graph()).await;
    let guard = make_resource_guard(manager);

    let cancel = CancellationToken::new();
    let (mut stream, meter) = metered_stream(inner, guard, cancel, None);

    while stream.next().await.is_some() {}

    let mut m = meter.lock().unwrap();
    let report = m.finalize(None);

    assert_eq!(
        report.completion_tokens, 5,
        "final chunk's usage should set completion_tokens"
    );
    assert_eq!(
        report.prompt_tokens, 10,
        "final chunk's usage should set prompt_tokens"
    );
    assert_eq!(
        report.total_tokens, 15,
        "total should be prompt + completion"
    );
}

// ---------------------------------------------------------------------------
// Test 2 — MeteredStream reports TTFB > 0 after a delay
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_metered_stream_reports_ttfb() {
    let chunks = make_chunks();
    let inner: Pin<Box<dyn Stream<Item = anyhow::Result<ChatStreamChunk>> + Send>> =
        Box::pin(stream::iter(chunks));

    let manager: Arc<dyn ResourceManager> =
        Arc::new(DefaultResourceManager::new(test_quota()));
    manager.try_reserve(&test_graph()).await;
    let guard = make_resource_guard(manager);

    let cancel = CancellationToken::new();
    let (mut stream, meter) = metered_stream(inner, guard, cancel, None);

    // Wait before consuming so TTFB is non-trivial
    tokio::time::sleep(Duration::from_millis(10)).await;

    while stream.next().await.is_some() {}

    let mut m = meter.lock().unwrap();
    let report = m.finalize(None);

    assert!(
        report.ttfb_ms.unwrap() > 0,
        "ttfb should be > 0 after a deliberate delay"
    );
}

// ---------------------------------------------------------------------------
// Test 3 — Cancelling a MeteredStream stops early
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_cancelling_stream_stops_early() {
    let inner: Pin<Box<dyn Stream<Item = anyhow::Result<ChatStreamChunk>> + Send>> =
        Box::pin(slow_chunks(100, Duration::from_millis(1)));

    let manager: Arc<dyn ResourceManager> =
        Arc::new(DefaultResourceManager::new(test_quota()));
    manager.try_reserve(&test_graph()).await;
    let guard = make_resource_guard(manager);

    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();

    let cancel_handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        cancel_clone.cancel();
    });

    let (mut stream, _meter) = metered_stream(inner, guard, cancel, None);

    let mut count = 0;
    while stream.next().await.is_some() {
        count += 1;
    }

    cancel_handle.await.unwrap();
    assert!(
        count < 100,
        "should have been cancelled before exhausting all 100 chunks (got {count})"
    );
    assert!(
        count > 0,
        "should have consumed at least one chunk before cancellation"
    );
}

// ---------------------------------------------------------------------------
// Test 4 — ResourceGuard is released when stream is exhausted
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_stream_resource_guard_released_on_exhaustion() {
    let graph = test_graph();
    let manager: Arc<dyn ResourceManager> =
        Arc::new(DefaultResourceManager::new(test_quota()));

    let reserved = manager.try_reserve(&graph).await;
    assert!(reserved, "reservation should succeed");
    assert_eq!(manager.spent_tokens(), 100);

    let chunks = make_chunks();
    let inner: Pin<Box<dyn Stream<Item = anyhow::Result<ChatStreamChunk>> + Send>> =
        Box::pin(stream::iter(chunks));

    let guard = ResourceGuard::new(Uuid::new_v4(), graph, manager.clone());
    let cancel = CancellationToken::new();
    let (mut stream, _meter) = metered_stream(inner, guard, cancel, None);

    while stream.next().await.is_some() {}

    // The guard's drop spawns an async release — yield so it can run
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(
        manager.spent_tokens(),
        0,
        "guard release should restore tokens to zero"
    );
}

// ---------------------------------------------------------------------------
// Test 5 — StreamMetrics counters increment on record_report
// ---------------------------------------------------------------------------

#[test]
fn test_streaming_metrics_increment() {
    let metrics = StreamMetrics::instance();
    let req_before = metrics.streaming_requests_total.get();
    let tok_before = metrics.streaming_tokens_total.get();

    let report = StreamMeterReport {
        prompt_tokens: 10,
        completion_tokens: 20,
        total_tokens: 30,
        cost: fusion_router::types::NanoUSD::from_nanos(5_000_000),
        ttfb_ms: Some(200),
        total_duration_ms: Some(5000),
    };

    metrics.record_report(&report);

    assert_eq!(
        metrics.streaming_requests_total.get(),
        req_before + 1,
        "request counter should increment by 1"
    );
    assert_eq!(
        metrics.streaming_tokens_total.get(),
        tok_before + 30,
        "token counter should increment by total_tokens"
    );
}

// ---------------------------------------------------------------------------
// Test 6 — StreamCheckpoint serializes and deserializes cleanly
// ---------------------------------------------------------------------------

#[test]
fn test_stream_checkpoint_round_trip() {
    let checkpoint = StreamCheckpoint {
        generation: 1,
        request_id: "req-123".to_string(),
        model: "gpt-4".to_string(),
        chunks_received: 42,
        content_so_far: "Hello world".to_string(),
        completion_tokens_accumulated: 50,
        timestamp: chrono::Utc::now(),
    };

    let json = serde_json::to_string(&checkpoint).unwrap();
    let deserialized: StreamCheckpoint = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.generation, 1);
    assert_eq!(deserialized.request_id, "req-123");
    assert_eq!(deserialized.model, "gpt-4");
    assert_eq!(deserialized.chunks_received, 42);
    assert_eq!(deserialized.content_so_far, "Hello world");
    assert_eq!(deserialized.completion_tokens_accumulated, 50);
}
