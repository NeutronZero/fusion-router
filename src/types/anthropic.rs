use serde::{Deserialize, Serialize};

/// Anthropic Messages API Request (`POST /v1/messages`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessagesRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    #[serde(default)]
    pub system: Option<AnthropicSystemPrompt>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicSystemPrompt {
    String(String),
    Blocks(Vec<AnthropicContentBlock>),
}

impl AnthropicSystemPrompt {
    pub fn to_text(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Blocks(blocks) => blocks
                .iter()
                .map(|b| match b {
                    AnthropicContentBlock::Text { text } => text.as_str(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicMessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicMessageContent {
    String(String),
    Blocks(Vec<AnthropicContentBlock>),
}

impl AnthropicMessageContent {
    pub fn to_text(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Blocks(blocks) => blocks
                .iter()
                .map(|b| match b {
                    AnthropicContentBlock::Text { text } => text.as_str(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
}

/// Anthropic Messages API Response (Non-Streaming)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessagesResponse {
    pub id: String,
    pub r#type: String, // "message"
    pub role: String,   // "assistant"
    pub model: String,
    pub content: Vec<AnthropicResponseContentBlock>,
    pub stop_reason: Option<String>, // "end_turn", "max_tokens", "stop_sequence"
    pub stop_sequence: Option<String>,
    pub usage: AnthropicUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicResponseContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl AnthropicMessagesRequest {
    pub fn into_chat_completion_request(self) -> crate::types::ChatCompletionRequest {
        let mut messages = Vec::new();

        // 1. System prompt (if provided) prepended as system message
        if let Some(sys) = self.system {
            let text = sys.to_text();
            if !text.trim().is_empty() {
                messages.push(crate::types::ChatMessage {
                    role: "system".to_string(),
                    content: text,
                });
            }
        }

        // 2. User & Assistant messages
        for msg in self.messages {
            messages.push(crate::types::ChatMessage {
                role: msg.role,
                content: msg.content.to_text(),
            });
        }

        crate::types::ChatCompletionRequest {
            model: self.model,
            messages,
            stream: self.stream,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            tools: None,
            files: None,
            execution: None,
            output: None,
            strategy: None,
        }
    }
}

impl From<(crate::types::ChatCompletionResponse, String)> for AnthropicMessagesResponse {
    fn from((resp, req_model): (crate::types::ChatCompletionResponse, String)) -> Self {
        let text_content = resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        let finish_reason = resp
            .choices
            .first()
            .map(|c| c.finish_reason.as_str())
            .unwrap_or("stop");

        let stop_reason = match finish_reason {
            "length" => "max_tokens",
            "stop" => "end_turn",
            _ => "end_turn",
        };

        let usage = resp.usage.unwrap_or(crate::types::Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        });

        AnthropicMessagesResponse {
            id: if resp.id.starts_with("msg_") {
                resp.id
            } else {
                format!("msg_{}", resp.id)
            },
            r#type: "message".to_string(),
            role: "assistant".to_string(),
            model: if resp.model.is_empty() {
                req_model
            } else {
                resp.model
            },
            content: vec![AnthropicResponseContentBlock::Text { text: text_content }],
            stop_reason: Some(stop_reason.to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: usage.prompt_tokens,
                output_tokens: usage.completion_tokens,
            },
        }
    }
}
