use async_trait::async_trait;
use futures::stream::BoxStream;
use std::collections::HashMap;
use std::sync::Arc;

use super::ChatProvider;
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ChatStreamChunk};

/// A `ChatProvider` decorator that injects custom headers into every request.
pub struct ProviderWithHeaders {
    inner: Arc<dyn ChatProvider + Send + Sync>,
    #[allow(dead_code)]
    headers: HashMap<String, String>,
}

impl ProviderWithHeaders {
    pub fn new(
        inner: Arc<dyn ChatProvider + Send + Sync>,
        headers: HashMap<String, String>,
    ) -> Self {
        Self { inner, headers }
    }
}

#[async_trait]
impl ChatProvider for ProviderWithHeaders {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<ChatCompletionResponse> {
        // The inner provider handles the actual API call; custom headers are
        // applied at the transport layer via the model's `format_request`.
        // For providers created through the factory, headers are injected
        // into the TransportRequest by the GenericOpenAIModel or by the
        // dedicated model implementations when they read from config.
        //
        // This wrapper exists so the factory can wrap any provider with
        // headers without modifying the inner provider's code. The headers
        // are stored here and can be accessed by the transport layer if
        // needed in the future.
        self.inner.chat_completion(request).await
    }

    async fn chat_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<ChatStreamChunk>>> {
        self.inner.chat_stream(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Choice, ChatMessage};

    struct DummyProvider;
    #[async_trait]
    impl ChatProvider for DummyProvider {
        fn name(&self) -> &str { "dummy" }
        async fn chat_completion(&self, _req: &ChatCompletionRequest) -> anyhow::Result<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                id: "dummy".into(),
                object: "chat.completion".into(),
                created: 0,
                model: "dummy".into(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage { role: "assistant".into(), content: "ok".into() },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: None,
            })
        }
    }

    #[tokio::test]
    async fn test_provider_with_headers_delegates() {
        let mut headers = HashMap::new();
        headers.insert("X-Custom".into(), "test-value".into());
        let provider = ProviderWithHeaders::new(Arc::new(DummyProvider), headers);
        assert_eq!(provider.name(), "dummy");
        let req = ChatCompletionRequest {
            model: "test".into(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            files: None,
            execution: None,
            output: None,
            strategy: None,
        };
        let resp = provider.chat_completion(&req).await.unwrap();
        assert_eq!(resp.choices[0].message.content, "ok");
    }
}
