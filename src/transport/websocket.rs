use async_trait::async_trait;

use crate::transport::{
    Transport, TransportError, TransportEvent, TransportRequest, TransportResponse,
};

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
        Err(TransportError::Network(format!(
            "WebSocket transport not yet implemented (would connect to {})",
            self.url
        )))
    }

    #[tracing::instrument(skip(self, _req))]
    async fn stream(
        &self,
        _req: TransportRequest,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<TransportEvent, TransportError>>,
        TransportError,
    > {
        Err(TransportError::Network(
            "WebSocket streaming not yet implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_new_stores_url() {
        let transport = WebSocketTransport::new("wss://example.com/socket".into());
        assert_eq!(transport.url, "wss://example.com/socket");
    }

    #[tokio::test]
    async fn test_send_reports_unimplemented_with_url() {
        let transport = WebSocketTransport::new("wss://example.com/socket".into());
        let req = TransportRequest {
            url: "wss://example.com/socket".into(),
            method: "POST".into(),
            headers: HashMap::new(),
            body: serde_json::json!({}),
        };

        let err = transport.send(req).await.unwrap_err();
        assert!(
            err.to_string().contains("wss://example.com/socket"),
            "error should mention the configured url, got: {err}"
        );
    }
}
