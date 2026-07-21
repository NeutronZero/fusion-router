use std::sync::Arc;
use async_trait::async_trait;
use crate::providers::circuit_breaker::CircuitBreaker;
use crate::providers::ChatProvider;
use crate::types::{ChatCompletionRequest, ChatCompletionResponse};

pub struct CircuitBreakingProvider {
    inner: Arc<dyn ChatProvider + Send + Sync>,
    breaker: CircuitBreaker,
    name: String,
}

impl CircuitBreakingProvider {
    pub fn new(
        inner: Arc<dyn ChatProvider + Send + Sync>,
        failure_threshold: u32,
        success_threshold: u32,
        cooldown_secs: u64,
        name: String,
    ) -> Self {
        Self {
            inner,
            breaker: CircuitBreaker::new(failure_threshold, success_threshold, cooldown_secs),
            name,
        }
    }

    pub fn breaker(&self) -> &CircuitBreaker {
        &self.breaker
    }
}

#[async_trait]
impl ChatProvider for CircuitBreakingProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat_completion(&self, request: &ChatCompletionRequest) -> anyhow::Result<ChatCompletionResponse> {
        if !self.breaker.can_execute() {
            return Err(anyhow::anyhow!("Circuit breaker is OPEN for provider: {}", self.name));
        }
        match self.inner.chat_completion(request).await {
            Ok(response) => {
                self.breaker.record_success();
                Ok(response)
            }
            Err(e) => {
                self.breaker.record_failure();
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChatMessage;
    use crate::types::Choice;

    struct MockOkProvider;
    #[async_trait]
    impl ChatProvider for MockOkProvider {
        fn name(&self) -> &str { "mock-ok" }
        async fn chat_completion(&self, _req: &ChatCompletionRequest) -> anyhow::Result<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                id: "test".into(), object: "chat.completion".into(), created: 0,
                model: "test".into(),
                choices: vec![Choice { index: 0, message: ChatMessage { role: "assistant".into(), content: "ok".into() }, finish_reason: "stop".into() }],
                usage: None,
            })
        }
    }

    struct MockFailProvider;
    #[async_trait]
    impl ChatProvider for MockFailProvider {
        fn name(&self) -> &str { "mock-fail" }
        async fn chat_completion(&self, _req: &ChatCompletionRequest) -> anyhow::Result<ChatCompletionResponse> {
            Err(anyhow::anyhow!("always fails"))
        }
    }

    #[tokio::test]
    async fn test_passes_through_success() {
        let wrapped = CircuitBreakingProvider::new(Arc::new(MockOkProvider), 3, 2, 5, "test".into());
        let req = ChatCompletionRequest {
            model: "test".into(), messages: vec![], stream: false,
            temperature: None, max_tokens: None, tools: None, files: None,
            execution: None, output: None,
        };
        let result = wrapped.chat_completion(&req).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_opens_after_threshold() {
        let wrapped = CircuitBreakingProvider::new(Arc::new(MockFailProvider), 3, 2, 5, "test".into());
        let req = ChatCompletionRequest {
            model: "test".into(), messages: vec![], stream: false,
            temperature: None, max_tokens: None, tools: None, files: None,
            execution: None, output: None,
        };
        for _ in 0..3 {
            let result = wrapped.chat_completion(&req).await;
            assert!(result.is_err());
        }
        let result = wrapped.chat_completion(&req).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Circuit breaker is OPEN"), "Expected circuit breaker error, got: {}", err);
    }
}
