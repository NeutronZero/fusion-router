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
    pub fn trim_messages(&self, messages: &[crate::types::ChatMessage], max_tokens: u32) -> Vec<crate::types::ChatMessage> {
        let total_tokens: u32 = messages.iter()
            .map(|m| estimate_tokens(&m.content))
            .sum();

        if total_tokens <= max_tokens {
            return messages.to_vec();
        }

        let mut system_msgs: Vec<crate::types::ChatMessage> = Vec::new();
        let mut other_msgs: Vec<crate::types::ChatMessage> = Vec::new();

        for msg in messages {
            if msg.role == "system" {
                system_msgs.push(msg.clone());
            } else {
                other_msgs.push(msg.clone());
            }
        }

        let system_tokens: u32 = system_msgs.iter()
            .map(|m| estimate_tokens(&m.content))
            .sum();

        let mut remaining = max_tokens.saturating_sub(system_tokens + 5);
        let mut trimmed_other: Vec<crate::types::ChatMessage> = Vec::new();

        for msg in other_msgs.iter().rev() {
            let tokens = estimate_tokens(&msg.content) + 5;
            if tokens <= remaining {
                trimmed_other.push(msg.clone());
                remaining -= tokens;
            } else if remaining > 10 {
                let truncated: String = msg.content.chars().take((remaining * 4) as usize).collect();
                trimmed_other.push(crate::types::ChatMessage {
                    role: msg.role.clone(),
                    content: truncated,
                });
                remaining = 0;
            }
        }

        trimmed_other.reverse();
        let mut result = system_msgs;
        result.extend(trimmed_other);
        result
    }
}

pub fn estimate_tokens(text: &str) -> u32 {
    (text.len() as u32 + 3) / 4
}
