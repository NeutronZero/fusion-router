use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::{Context, Poll};
use futures::Stream;
use tokio_util::sync::CancellationToken;

use crate::resource::guard::ResourceGuard;
use crate::resource::stream_meter::{StreamMeter, StreamMeterReport};
use crate::providers::ModelPricing;
use crate::types::ChatStreamChunk;

/// Fired exactly once when a streamed response terminates (completion,
/// error, cancellation, or drop) with the final measured report. Used to
/// record exact usage into the resource manager so quota accounting reflects
/// what actually streamed rather than the admission estimate alone.
pub type StreamFinishHook = Box<dyn FnOnce(StreamMeterReport) + Send + 'static>;

pub struct CancellingStream<S> {
    inner: S,
    cancel: CancellationToken,
}

impl<S> CancellingStream<S> {
    pub fn new(inner: S, cancel: CancellationToken) -> Self {
        Self { inner, cancel }
    }
}

impl<S, T> Stream for CancellingStream<S>
where
    S: Stream<Item = T> + Unpin,
{
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.cancel.is_cancelled() {
            return Poll::Ready(None);
        }
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

pub struct MeteredStream {
    inner: Pin<Box<dyn Stream<Item = anyhow::Result<ChatStreamChunk>> + Send>>,
    cancel: CancellationToken,
    meter: Arc<Mutex<StreamMeter>>,
    guard: Option<ResourceGuard>,
    pricing: Option<ModelPricing>,
    on_finish: Option<StreamFinishHook>,
}

impl Stream for MeteredStream {
    type Item = anyhow::Result<ChatStreamChunk>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.cancel.is_cancelled() {
            self.release_guard();
            self.fire_finish_hook();
            return Poll::Ready(None);
        }
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                if let Ok(mut meter) = self.meter.lock() {
                    meter.record_chunk(&chunk, self.pricing.as_ref());
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(e))) => {
                self.release_guard();
                self.fire_finish_hook();
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                if let Ok(mut meter) = self.meter.lock() {
                    meter.finalize(self.pricing.as_ref());
                }
                self.release_guard();
                self.fire_finish_hook();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl MeteredStream {
    fn release_guard(&mut self) {
        if let Some(guard) = self.guard.take() {
            drop(guard);
        }
    }

    fn fire_finish_hook(&mut self) {
        if let Some(hook) = self.on_finish.take() {
            let report = self
                .meter
                .lock()
                .map(|mut m| m.finalize(self.pricing.as_ref()))
                .unwrap_or_else(|_| StreamMeterReport {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                    cost_millicosts: 0,
                    ttfb_ms: None,
                    total_duration_ms: None,
                });
            hook(report);
        }
    }
}

impl Drop for MeteredStream {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.release_guard();
        self.fire_finish_hook();
    }
}

pub fn metered_stream(
    inner: Pin<Box<dyn Stream<Item = anyhow::Result<ChatStreamChunk>> + Send>>,
    guard: ResourceGuard,
    cancel: CancellationToken,
    pricing: Option<ModelPricing>,
) -> (MeteredStream, Arc<Mutex<StreamMeter>>) {
    metered_stream_with_finish(inner, guard, cancel, pricing, Box::new(|_| {}))
}

/// Same as [`metered_stream`] but invokes `on_finish` exactly once when the
/// stream terminates, with the final measured report.
pub fn metered_stream_with_finish(
    inner: Pin<Box<dyn Stream<Item = anyhow::Result<ChatStreamChunk>> + Send>>,
    guard: ResourceGuard,
    cancel: CancellationToken,
    pricing: Option<ModelPricing>,
    on_finish: StreamFinishHook,
) -> (MeteredStream, Arc<Mutex<StreamMeter>>) {
    let meter = Arc::new(Mutex::new(StreamMeter::new()));
    let meter_clone = meter.clone();
    let stream = MeteredStream {
        inner,
        cancel,
        meter: meter_clone,
        guard: Some(guard),
        pricing,
        on_finish: Some(on_finish),
    };
    (stream, meter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use futures::stream;
    use futures::StreamExt;
    use uuid::Uuid;
    use crate::resource::DefaultResourceManager;
    use crate::types::{ExecutionGraph, GraphMetadata, Quota};
    use std::collections::HashMap;

    #[test]
    fn test_cancelling_stream_passes_through_normal() {
        let cancel = CancellationToken::new();
        let inner = stream::iter(vec![1, 2, 3]);
        let mut stream = CancellingStream::new(inner, cancel);
        let mut collected = vec![];
        while let Some(item) = block_on(stream.next()) {
            collected.push(item);
        }
        assert_eq!(collected, vec![1, 2, 3]);
    }

    #[test]
    fn test_cancelling_stream_returns_none_on_cancel() {
        let cancel = CancellationToken::new();
        let inner = stream::iter(vec![1, 2, 3]);
        let mut stream = CancellingStream::new(inner, cancel.clone());
        cancel.cancel();
        assert_eq!(block_on(stream.next()), None);
    }

    fn make_test_guard() -> ResourceGuard {
        let quota = Quota {
            max_daily_cost: 100.0,
            max_daily_tokens: 100000,
            max_concurrent: 10,
            provider_limits: HashMap::new(),
        };
        let manager: Arc<dyn crate::resource::ResourceManager> =
            Arc::new(DefaultResourceManager::new(quota));
        let graph = ExecutionGraph {
            graph_id: Uuid::new_v4(),
            nodes: vec![],
            edges: vec![],
            metadata: GraphMetadata {
                estimated_cost: 0.0,
                estimated_tokens: 0,
                max_depth: 0,
                node_count: 0,
            },
            total_tokens: 0,
            total_cost: 0,
            primitive_graph_hash: 0,
        };
        ResourceGuard::new(Uuid::new_v4(), graph, manager)
    }

    #[test]
    fn test_metered_stream_records_chunks() {
        let cancel = CancellationToken::new();
        let guard = make_test_guard();
        let chunk = ChatStreamChunk {
            content: Some("Hello world".to_string()),
            finish_reason: None,
            usage: None,
        };
        let inner: Pin<Box<dyn Stream<Item = anyhow::Result<ChatStreamChunk>> + Send>> = Box::pin(
            stream::once(futures::future::ready(Ok(chunk))),
        );
        let (mut stream, meter) = metered_stream(inner, guard, cancel, None);
        let result = block_on(stream.next());
        assert!(result.is_some());
        assert!(result.unwrap().is_ok());
        let meter = meter.lock().unwrap();
        assert_eq!(meter.completion_tokens(), 3);
    }

    #[test]
    fn test_metered_stream_passes_chunks_through() {
        let cancel = CancellationToken::new();
        let guard = make_test_guard();
        let chunk1 = ChatStreamChunk {
            content: Some("Hello".to_string()),
            finish_reason: None,
            usage: None,
        };
        let chunk2 = ChatStreamChunk {
            content: Some(" world".to_string()),
            finish_reason: None,
            usage: None,
        };
        let inner: Pin<Box<dyn Stream<Item = anyhow::Result<ChatStreamChunk>> + Send>> = Box::pin(
            stream::iter(vec![Ok(chunk1.clone()), Ok(chunk2.clone())]),
        );
        let (mut stream, _meter) = metered_stream(inner, guard, cancel, None);
        let result1 = block_on(stream.next()).unwrap().unwrap();
        assert_eq!(result1.content, chunk1.content);
        let result2 = block_on(stream.next()).unwrap().unwrap();
        assert_eq!(result2.content, chunk2.content);
        let result3 = block_on(stream.next());
        assert!(result3.is_none());
    }
}
