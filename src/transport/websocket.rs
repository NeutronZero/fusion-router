use async_trait::async_trait;

use super::{Transport, TransportStream};

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
    async fn send(&self, request: &str) -> Result<String, String> {
        Err(format!("WebSocket transport not yet implemented (would connect to {} with: {})", self.url, request))
    }

    async fn stream(&self, _request: &str) -> Result<Box<dyn TransportStream>, String> {
        Err("WebSocket streaming not yet implemented".to_string())
    }
}
