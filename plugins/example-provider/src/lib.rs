use std::ffi::{CString, c_void};

/// Plugin entry point: creates a provider instance.
/// Returns a heap-allocated raw pointer to the provider.
#[no_mangle]
pub extern "C" fn plugin_create_provider() -> *mut c_void {
    let provider = Box::new(ExampleProvider);
    Box::into_raw(provider) as *mut c_void
}

/// Plugin entry point: destroys a provider instance.
#[no_mangle]
pub extern "C" fn plugin_destroy_provider(ptr: *mut c_void) {
    if !ptr.is_null() {
        unsafe {
            let _ = Box::from_raw(ptr as *mut ExampleProvider);
        }
    }
}

/// Plugin entry point: returns the provider's name as a C string.
#[no_mangle]
pub extern "C" fn plugin_provider_name() -> *const std::ffi::c_char {
    let name = CString::new("example-provider").unwrap();
    name.into_raw()
}

pub struct ExampleProvider;

#[async_trait::async_trait]
impl ChatProvider for ExampleProvider {
    fn name(&self) -> &str {
        "example-provider"
    }

    async fn chat_completion(
        &self,
        _request: &ChatCompletionRequest,
    ) -> anyhow::Result<ChatCompletionResponse> {
        Ok(ChatCompletionResponse {
            id: "example-id".to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: _request.model.clone(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: "Hello from example provider plugin!".to_string(),
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        })
    }
}

// Type stubs so this crate compiles independently
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub files: Option<Vec<FileRef>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRef {
    pub name: String,
    pub content: String,
    pub mime_type: Option<String>,
}

#[async_trait::async_trait]
pub trait ChatProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn chat_completion(&self, request: &ChatCompletionRequest) -> anyhow::Result<ChatCompletionResponse>;
}
