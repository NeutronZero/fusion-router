use async_trait::async_trait;

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, request: &str) -> Result<String, String>;
    async fn stream(&self, request: &str) -> Result<Box<dyn TransportStream>, String>;
}

#[async_trait]
pub trait TransportStream: Send {
    async fn next(&mut self) -> Option<String>;
}

pub mod http;
pub mod websocket;
pub mod stdio;

pub use http::HttpTransport;
