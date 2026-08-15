//! Bridge between the src provider interfaces and `fusion_runtime`
//! (Phase 6.4 executor delegation).
//!
//! - `FusionChatProvider` adapts `crate::providers::ChatProvider` to the
//!   `fusion_runtime::ChatProvider` contract, preserving the failure wording
//!   (`"Provider error: …"`) the src executor has always surfaced.
//!
//! The bridge is cheap: no provider state, only `Arc` clones, so it may be
//! rebuilt per node execution without meaningful overhead.

use std::sync::Arc;

use async_trait::async_trait;

use crate::providers::ChatProvider;
use crate::types::ChatCompletionRequest;

/// Adapts a src `ChatProvider` to the `fusion_runtime::ChatProvider` contract.
pub struct FusionChatProvider {
    inner: Arc<dyn ChatProvider + Send + Sync>,
    #[cfg(feature = "semantic-cache")]
    cache: Option<Arc<crate::cache::SemanticCache>>,
}

impl FusionChatProvider {
    pub fn new(inner: Arc<dyn ChatProvider + Send + Sync>) -> Self {
        Self {
            inner,
            #[cfg(feature = "semantic-cache")]
            cache: None,
        }
    }

    #[cfg(feature = "semantic-cache")]
    pub fn with_cache(mut self, cache: Option<Arc<crate::cache::SemanticCache>>) -> Self {
        self.cache = cache;
        self
    }

    #[cfg(feature = "semantic-cache")]
    fn cache_key(model: &str, messages: &[fusion_runtime::ChatMessage]) -> String {
        let messages_json = serde_json::to_string(messages).unwrap_or_default();
        format!("{model}:{messages_json}")
    }

    fn to_src_request(request: &fusion_runtime::ChatRequest) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: request.model.clone(),
            messages: request
                .messages
                .iter()
                .map(|m| crate::types::ChatMessage {
                    role: m.role.clone(),
                    content: m.content.clone(),
                })
                .collect(),
            stream: false,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            tools: None,
            files: None,
            execution: None,
            output: None,
            strategy: None,
        }
    }

    fn to_runtime_response(
        response: &crate::types::ChatCompletionResponse,
    ) -> fusion_runtime::ChatResponse {
        let content = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        fusion_runtime::ChatResponse {
            content,
            usage: response.usage.clone(),
            tool_calls: response.native_tool_calls.clone().unwrap_or_default(),
            tool_results: Vec::new(),
        }
    }
}

#[async_trait]
impl fusion_runtime::ChatProvider for FusionChatProvider {
    async fn chat_completion(
        &self,
        request: &fusion_runtime::ChatRequest,
    ) -> Result<fusion_runtime::ChatResponse, String> {
        #[cfg(feature = "semantic-cache")]
        if let Some(ref cache) = self.cache {
            let key = Self::cache_key(&request.model, &request.messages);
            if let Some(cached) = cache.get(&key).await {
                if let Some(content) = cached
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                {
                    return Ok(fusion_runtime::ChatResponse {
                        content,
                        usage: None,
                        tool_calls: Vec::new(),
                        tool_results: Vec::new(),
                    });
                }
            }
        }

        let src_request = Self::to_src_request(request);
        match self.inner.chat_completion(&src_request).await {
            Ok(response) => {
                #[cfg(feature = "semantic-cache")]
                if let Some(ref cache) = self.cache {
                    let content = response
                        .choices
                        .first()
                        .map(|c| c.message.content.clone())
                        .unwrap_or_default();
                    if !content.trim().is_empty() {
                        let key = Self::cache_key(&request.model, &request.messages);
                        cache
                            .put(&key, serde_json::json!({ "content": content }))
                            .await;
                    }
                }
                Ok(Self::to_runtime_response(&response))
            }
            Err(e) => Err(format!("Provider error: {e}")),
        }
    }
}

/// Re-exports the `fusion_runtime::ChatProvider` trait alias so callers do
/// not need to spell the fully-qualified path twice.
pub use fusion_runtime::ChatProvider as FusionRuntimeChatProvider;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    struct CountingProvider(Arc<std::sync::atomic::AtomicUsize>);

    #[async_trait]
    impl ChatProvider for CountingProvider {
        fn name(&self) -> &str {
            "counting"
        }

        async fn chat_completion(
            &self,
            request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ChatCompletionResponse {
                id: "c".into(),
                object: "chat.completion".into(),
                created: 0,
                model: request.model.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "bridge response".into(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: Some(Usage {
                    prompt_tokens: 7,
                    completion_tokens: 3,
                    total_tokens: 10,
                }),
            })
        }
    }

    #[tokio::test]
    async fn maps_request_and_response() {
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let bridge = FusionChatProvider::new(Arc::new(CountingProvider(counter.clone())));
        let response = bridge
            .chat_completion(&fusion_runtime::ChatRequest {
                model: "gpt-4".into(),
                messages: vec![fusion_runtime::ChatMessage {
                    role: "user".into(),
                    content: "hi".into(),
                }],
                temperature: Some(0.5),
                max_tokens: Some(64),
            })
            .await
            .expect("chat");
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(response.content, "bridge response");
        assert_eq!(response.usage.unwrap().total_tokens, 10);
        assert!(response.tool_calls.is_empty());
    }

    #[tokio::test]
    async fn provider_error_keeps_src_wording() {
        struct Failing;
        #[async_trait]
        impl ChatProvider for Failing {
            fn name(&self) -> &str {
                "failing"
            }
            async fn chat_completion(
                &self,
                _request: &ChatCompletionRequest,
            ) -> anyhow::Result<ChatCompletionResponse> {
                anyhow::bail!("provider exploded")
            }
        }

        let bridge = FusionChatProvider::new(Arc::new(Failing));
        let err = bridge
            .chat_completion(&fusion_runtime::ChatRequest {
                model: "m".into(),
                messages: vec![],
                temperature: None,
                max_tokens: None,
            })
            .await
            .expect_err("must fail");
        assert_eq!(err, "Provider error: provider exploded");
    }
}
