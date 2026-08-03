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
                let byte_limit = (remaining * 4) as usize;
                let safe_end = msg.content.char_indices()
                    .map(|(i, c)| i + c.len_utf8())
                    .take_while(|&i| i <= byte_limit)
                    .last()
                    .unwrap_or(0);
                let truncated: String = msg.content[..safe_end].to_string();
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
    (text.len() as u32).div_ceil(4)
}

#[cfg(test)]
mod tests {
    use crate::context::assembler::DefaultContextAssembler;
    use crate::types::ChatMessage;

    #[test]
    fn test_trim_respects_token_budget_with_multibyte_utf8() {
        let assembler = DefaultContextAssembler::new();
        // "你好世界" is 4 chars × 3 bytes = 12 bytes → estimate_tokens = 3
        // Repeated 100× = 400 chars, 1200 bytes, ~300 tokens
        let content = "你好世界".repeat(100);
        let messages = vec![ChatMessage {
            role: "user".into(),
            content,
        }];
        // max_tokens = 20 → forces aggressive trimming
        let result = assembler.trim_messages(&messages, 20);
        let total: u32 = result.iter().map(|m| crate::context::assembler::estimate_tokens(&m.content)).sum();
        assert!(
            total <= 20,
            "trimmed multi-byte content exceeds budget of 20 tokens, computed {}",
            total,
        );
    }

    #[test]
    fn test_estimate_tokens_basic() {
        assert_eq!(super::estimate_tokens(""), 0);
        assert_eq!(super::estimate_tokens("a"), 1);
        assert_eq!(super::estimate_tokens("abcd"), 1);
        assert_eq!(super::estimate_tokens("abcde"), 2);
        assert_eq!(super::estimate_tokens("hello world"), 3);
        assert_eq!(super::estimate_tokens("你好世界"), 3);
    }

    #[test]
    fn test_full_budget_no_trimming() {
        let assembler = DefaultContextAssembler::new();
        let messages = vec![
            ChatMessage { role: "system".into(), content: "sys".into() },
            ChatMessage { role: "user".into(), content: "hello".into() },
        ];
        let result = assembler.trim_messages(&messages, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "sys");
        assert_eq!(result[1].content, "hello");
    }

    #[test]
    fn test_trim_reverse_chronological() {
        let assembler = DefaultContextAssembler::new();
        let messages = vec![
            ChatMessage { role: "system".into(), content: "sys".into() },
            ChatMessage { role: "user".into(), content: "A".repeat(100) },
            ChatMessage { role: "user".into(), content: "B".repeat(10) },
        ];
        let result = assembler.trim_messages(&messages, 15);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, "system");
        assert_eq!(result[1].content, "B".repeat(10));
    }

    #[test]
    fn test_empty_messages_handling() {
        let assembler = DefaultContextAssembler::new();
        let messages = vec![
            ChatMessage { role: "system".into(), content: "".into() },
            ChatMessage { role: "user".into(), content: "".into() },
            ChatMessage { role: "assistant".into(), content: "   ".into() },
        ];
        let result = assembler.trim_messages(&messages, 10);
        assert_eq!(result.len(), 3);
        let result = assembler.trim_messages(&messages, 0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "system");
    }
}
