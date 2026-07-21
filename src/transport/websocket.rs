use async_trait::async_trait;

use crate::transport::{Transport, TransportRequest, TransportResponse, TransportEvent, TransportError};

pub struct WebSocketTransport {
    url: String,
}

impl WebSocketTransport {
    pub fn new(url: String) -> Self {
        Self { url }
    }
}

#[async_trait]
impl Transport for WebSocketTransport {
    #[tracing::instrument(skip(self, _req))]
    async fn send(&self, _req: TransportRequest) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Network(format!("WebSocket transport not yet implemented (would connect to {})", self.url)))
    }

    #[tracing::instrument(skip(self, _req))]
    async fn stream(&self, _req: TransportRequest) -> Result<futures::stream::BoxStream<'static, Result<TransportEvent, TransportError>>, TransportError> {
        Err(TransportError::Network("WebSocket streaming not yet implemented".to_string()))
    }
}
