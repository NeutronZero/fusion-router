use async_trait::async_trait;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

pub mod zen_model;
pub mod openrouter_model;
pub mod ollama_model;
pub mod router;
pub mod ollama;
pub mod zen;
pub mod openrouter;

use crate::types::{ChatCompletionRequest, ChatCompletionResponse};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub coding_score: f32,
    pub reasoning_score: f32,
    pub max_context_tokens: u32,
    pub supports_tools: bool,
    pub supports_streaming: bool,
    pub supports_vision: bool,
    pub supports_json_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input_cost_per_1k: f64,
    pub output_cost_per_1k: f64,
}

#[async_trait]
pub trait Model: Send + Sync {
    fn id(&self) -> &str;
    fn provider_name(&self) -> &str;
    fn capabilities(&self) -> ModelCapabilities;
    fn pricing(&self) -> ModelPricing;
    fn quota_remaining(&self) -> Option<f64>;
    
    // Method to format a request for this model
    fn format_request(&self, req: &ChatCompletionRequest, api_key: &str) -> anyhow::Result<TransportRequest>;
    
    // Method to normalize a response for this model
    fn normalize_response(&self, resp: TransportResponse) -> anyhow::Result<ChatCompletionResponse>;
}

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

#[async_trait]
pub trait ChatProvider: Send + Sync {
    async fn chat_completion(&self, request: &ChatCompletionRequest) -> anyhow::Result<ChatCompletionResponse>;
    fn name(&self) -> &str;
}

pub struct Provider {
    pub model: Box<dyn Model>,
    pub transport: Box<dyn Transport>,
    pub api_key: String,
}

impl Provider {
    pub fn new(model: Box<dyn Model>, transport: Box<dyn Transport>, api_key: String) -> Self {
        Self {
            model,
            transport,
            api_key,
        }
    }
}

#[async_trait]
impl ChatProvider for Provider {
    fn name(&self) -> &str {
        self.model.provider_name()
    }

    async fn chat_completion(&self, request: &ChatCompletionRequest) -> anyhow::Result<ChatCompletionResponse> {
        let req = self.model.format_request(request, &self.api_key)?;
        let resp = self.transport.send(req).await.map_err(|e| anyhow::anyhow!("Transport error: {}", e))?;
        self.model.normalize_response(resp)
    }
}
