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
