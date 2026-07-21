use async_trait::async_trait;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

pub mod backoff;
pub mod http;
pub mod websocket;
pub mod stdio;

pub use http::HttpTransport;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportRequest {
    pub url: String,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub body: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportResponse {
    pub status: u16,
    pub body: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportEvent {
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum TransportError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("Timeout error: {0}")]
    Timeout(String),
    #[error("HTTP error status {status}: {body}")]
    Http {
        status: u16,
        body: String,
    },
    #[error("Serialization error: {0}")]
    Serialization(String),
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, req: TransportRequest) -> Result<TransportResponse, TransportError>;
    async fn stream(&self, req: TransportRequest) -> Result<futures::stream::BoxStream<'static, Result<TransportEvent, TransportError>>, TransportError>;
}
