use async_trait::async_trait;

use crate::types::{ChatCompletionRequest, ContextSnapshot};

#[async_trait]
pub trait ContextAssembler: Send + Sync {
    async fn assemble(&self, request: &ChatCompletionRequest) -> anyhow::Result<ContextSnapshot>;
}

pub struct DefaultContextAssembler {
    pub max_tokens: u32,
    pub default_temperature: f32,
}

impl DefaultContextAssembler {
    pub fn new() -> Self {
        Self {
            max_tokens: 4096,
            default_temperature: 0.7,
        }
    }
}

impl Default for DefaultContextAssembler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContextAssembler for DefaultContextAssembler {
    async fn assemble(&self, request: &ChatCompletionRequest) -> anyhow::Result<ContextSnapshot> {
        let messages = request.messages.clone();
        let files = request.files.clone().unwrap_or_default();
        let tools = request.tools.clone().unwrap_or_default();
        let max_tokens = request.max_tokens.unwrap_or(self.max_tokens);
        let temperature = request.temperature.unwrap_or(self.default_temperature);

        let trimmed = self.trim_messages(&messages, max_tokens);

        Ok(ContextSnapshot {
            messages: trimmed,
            files,
            tools,
            max_tokens,
            temperature,
        })
    }
}

impl DefaultContextAssembler {
    fn trim_messages(&self, messages: &[crate::types::ChatMessage], _max_tokens: u32) -> Vec<crate::types::ChatMessage> {
        messages.to_vec()
    }
}
