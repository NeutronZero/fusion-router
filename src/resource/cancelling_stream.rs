use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::{Context, Poll};
use futures::Stream;
use tokio_util::sync::CancellationToken;

use crate::resource::guard::ResourceGuard;
use crate::resource::stream_meter::StreamMeter;
use crate::providers::ModelPricing;
use crate::types::ChatStreamChunk;

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
}

impl Stream for MeteredStream {
    type Item = anyhow::Result<ChatStreamChunk>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.cancel.is_cancelled() {
            self.release_guard();
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
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                if let Ok(mut meter) = self.meter.lock() {
                    meter.finalize(self.pricing.as_ref());
                }
                self.release_guard();
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
}

impl Drop for MeteredStream {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.release_guard();
    }
}

pub fn metered_stream(
    inner: Pin<Box<dyn Stream<Item = anyhow::Result<ChatStreamChunk>> + Send>>,
    guard: ResourceGuard,
    cancel: CancellationToken,
    pricing: Option<ModelPricing>,
) -> (MeteredStream, Arc<Mutex<StreamMeter>>) {
    let meter = Arc::new(Mutex::new(StreamMeter::new()));
    let meter_clone = meter.clone();
    let stream = MeteredStream {
        inner,
        cancel,
        meter: meter_clone,
        guard: Some(guard),
        pricing,
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
