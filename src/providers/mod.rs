use async_trait::async_trait;

pub mod zen;
pub mod openrouter;
pub mod router;

use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ProviderRequest, ProviderResponse};

#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat_completion(&self, request: &ChatCompletionRequest) -> anyhow::Result<ChatCompletionResponse>;
    fn name(&self) -> &str;
}

#[async_trait]
pub trait Model: Send + Sync {
    fn model_name(&self) -> &str;
    fn provider_name(&self) -> &str;
    async fn generate(&self, request: &ProviderRequest) -> anyhow::Result<ProviderResponse>;
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, request: &ProviderRequest) -> anyhow::Result<ProviderResponse>;
}
