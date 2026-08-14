use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use tokio::sync::OnceCell;

use super::circuit_breaker::{CircuitBreaker, CircuitState};
use super::ChatProvider;
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ChatStreamChunk};

pub struct ProviderTarget {
    pub name: String,
    breaker: CircuitBreaker,
    factory: Box<dyn Fn() -> Arc<dyn ChatProvider + Send + Sync> + Send + Sync>,
    instance: OnceCell<Arc<dyn ChatProvider + Send + Sync>>,
}

impl ProviderTarget {
    pub fn new(
        name: String,
        breaker: CircuitBreaker,
        factory: Box<dyn Fn() -> Arc<dyn ChatProvider + Send + Sync> + Send + Sync>,
    ) -> Self {
        Self {
            name,
            breaker,
            factory,
            instance: OnceCell::new(),
        }
    }

    pub async fn get_or_init(&self) -> anyhow::Result<Arc<dyn ChatProvider + Send + Sync>> {
        Ok(self
            .instance
            .get_or_init(|| async { (self.factory)() })
            .await
            .clone())
    }

    pub fn can_execute(&self) -> bool {
        self.breaker.can_execute()
    }

    pub fn record_success(&self) {
        self.breaker.record_success();
    }

    pub fn record_failure(&self) {
        self.breaker.record_failure();
    }

    pub fn breaker_state(&self) -> CircuitState {
        self.breaker.state()
    }
}

pub struct ProviderRouter {
    targets: Vec<(Vec<String>, Arc<ProviderTarget>)>,
    default: Arc<ProviderTarget>,
}

impl ProviderRouter {
    pub fn new(default: ProviderTarget) -> Self {
        Self {
            targets: Vec::new(),
            default: Arc::new(default),
        }
    }

    pub fn with_provider(mut self, model_prefixes: Vec<String>, target: ProviderTarget) -> Self {
        self.targets.push((model_prefixes, Arc::new(target)));
        self
    }

    fn matching_targets(&self, model: &str) -> Vec<&ProviderTarget> {
        let mut matched = Vec::new();
        for (prefixes, target) in &self.targets {
            for prefix in prefixes {
                if model.starts_with(prefix) {
                    matched.push(target.as_ref());
                    break;
                }
            }
        }
        matched
    }

    pub fn resolve_target(
        &self,
        model: &str,
        model_reqs: Option<&crate::providers::ModelRequirements>,
        registry: Option<&crate::providers::registry::ProviderRegistry>,
    ) -> Vec<Arc<ProviderTarget>> {
        let matched = self.matching_targets(model);
        if !matched.is_empty() {
            // Return Arc clones of the matched targets
            return self.targets.iter()
                .filter(|(prefixes, _)| {
                    prefixes.iter().any(|p| model.starts_with(p))
                })
                .map(|(_, target)| target.clone())
                .collect();
        }

        // Fall back to capability-based selection
        if let (Some(reqs), Some(reg)) = (model_reqs, registry) {
            let candidates = reg.select_targets(reqs);
            if !candidates.is_empty() {
                return candidates;
            }
        }

        vec![self.default.clone()]
    }
}

#[async_trait]
impl ChatProvider for ProviderRouter {
    fn name(&self) -> &str {
        "router"
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<ChatCompletionResponse> {
        let matched = self.matching_targets(&request.model);
        let targets_to_try: Vec<&ProviderTarget> = if matched.is_empty() {
            vec![self.default.as_ref()]
        } else {
            matched
        };

        let mut last_error: Option<anyhow::Error> = None;

        for target in targets_to_try {
            if !target.can_execute() {
                tracing::warn!(
                    provider = %target.name,
                    "circuit is open, skipping"
                );
                continue;
            }

            tracing::debug!(model = %request.model, target = %target.name, "routing request");

            let provider = match target.get_or_init().await {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(
                        provider = %target.name,
                        error = %e,
                        "failed to lazily instantiate provider"
                    );
                    target.record_failure();
                    last_error = Some(e);
                    continue;
                }
            };

            match provider.chat_completion(request).await {
                Ok(response) => {
                    target.record_success();
                    return Ok(response);
                }
                Err(e) => {
                    tracing::warn!(
                        provider = %target.name,
                        error = %e,
                        "provider failed, trying next"
                    );
                    target.record_failure();
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no available providers")))
    }

    async fn chat_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<ChatStreamChunk>>> {
        let matched = self.matching_targets(&request.model);
        let targets_to_try: Vec<&ProviderTarget> = if matched.is_empty() {
            vec![self.default.as_ref()]
        } else {
            matched
        };

        let mut last_error: Option<anyhow::Error> = None;

        for target in targets_to_try {
            if !target.can_execute() {
                tracing::warn!(
                    provider = %target.name,
                    "circuit is open, skipping"
                );
                continue;
            }

            tracing::debug!(model = %request.model, target = %target.name, "routing streaming request");

            let provider = match target.get_or_init().await {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(
                        provider = %target.name,
                        error = %e,
                        "failed to lazily instantiate provider"
                    );
                    target.record_failure();
                    last_error = Some(e);
                    continue;
                }
            };

            match provider.chat_stream(request).await {
                Ok(stream) => {
                    target.record_success();
                    return Ok(stream);
                }
                Err(e) => {
                    tracing::warn!(
                        provider = %target.name,
                        error = %e,
                        "provider stream failed, trying next"
                    );
                    target.record_failure();
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no available providers")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatMessage, Choice};

    struct MockOkProvider;
    #[async_trait]
    impl ChatProvider for MockOkProvider {
        fn name(&self) -> &str {
            "mock-ok"
        }
        async fn chat_completion(
            &self,
            _req: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                id: "test".into(),
                object: "chat.completion".into(),
                created: 0,
                model: "mock-ok".into(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "ok".into(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: None,
            })
        }
    }

    struct MockFailProvider;
    #[async_trait]
    impl ChatProvider for MockFailProvider {
        fn name(&self) -> &str {
            "mock-fail"
        }
        async fn chat_completion(
            &self,
            _req: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            Err(anyhow::anyhow!("always fails"))
        }
    }

    fn mock_target(name: &str, succeed: bool) -> ProviderTarget {
        let factory: Box<dyn Fn() -> Arc<dyn ChatProvider + Send + Sync> + Send + Sync> = if succeed {
            Box::new(move || Arc::new(MockOkProvider) as Arc<dyn ChatProvider + Send + Sync>)
        } else {
            Box::new(move || Arc::new(MockFailProvider) as Arc<dyn ChatProvider + Send + Sync>)
        };
        ProviderTarget::new(name.into(), CircuitBreaker::new(3, 2, 5), factory)
    }

    fn dummy_request(model: &str) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: model.into(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            files: None,
            execution: None,
            output: None,
            strategy: None,
        }
    }

    #[tokio::test]
    async fn test_router_falls_through_prefixed_targets() {
        let router = ProviderRouter::new(mock_target("default", true))
            .with_provider(vec!["a/".into()], mock_target("fail-once", false))
            .with_provider(vec!["a/".into()], mock_target("ok", true));

        let res = router.chat_completion(&dummy_request("a/test")).await;
        assert!(res.is_ok(), "Should fall through from failing to succeeding provider");
    }

    #[tokio::test]
    async fn test_router_skips_open_circuit() {
        let breaker = CircuitBreaker::new(1, 2, 60);
        breaker.record_failure();

        let primary = ProviderTarget::new(
            "open".into(),
            breaker,
            Box::new(move || Arc::new(MockFailProvider) as Arc<dyn ChatProvider + Send + Sync>),
        );
        let secondary = mock_target("healthy", true);

        let router = ProviderRouter::new(mock_target("default", true))
            .with_provider(vec!["a/".into()], primary)
            .with_provider(vec!["a/".into()], secondary);

        let res = router.chat_completion(&dummy_request("a/test")).await;
        assert!(res.is_ok(), "Should skip open circuit and use fallback");
    }

    #[tokio::test]
    async fn test_router_uses_default_when_no_prefix_matches() {
        let router = ProviderRouter::new(mock_target("default", true))
            .with_provider(vec!["other/".into()], mock_target("other", false));

        let res = router.chat_completion(&dummy_request("unknown/model")).await;
        assert!(res.is_ok(), "Default provider should handle unmatched models");
    }

    #[test]
    fn test_resolve_target_prefix_match_wins() {
        let router = ProviderRouter::new(mock_target("default", true))
            .with_provider(vec!["known/".into()], mock_target("known-target", true));

        let result = router.resolve_target("known/model", None, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "known-target");
    }

    #[test]
    fn test_resolve_target_fallsback_to_registry() {
        let router = ProviderRouter::new(mock_target("default", true));

        let registry = crate::providers::registry::ProviderRegistry::new(mock_target("default", true));
        registry.register_target_with_capabilities(
            vec!["fallback/".into()],
            mock_target("fallback-model", true),
            crate::providers::ModelCapabilities {
                coding_score: 0.5, reasoning_score: 0.5, max_context_tokens: 32_000, max_output_tokens: 0,
                supports_tools: false, supports_streaming: true, supports_vision: false,
                supports_audio: false, supports_pdf: false, supports_json_mode: true,
                supports_thinking: false, supports_parallel_tools: false, supports_structured_output: false,
            },
            crate::providers::ModelPricing { input_cost_per_1k: crate::types::NanoUSD::from_nanos(150_000_000), output_cost_per_1k: crate::types::NanoUSD::from_nanos(600_000_000) },
        );

        let reqs = crate::providers::ModelRequirements {
            requires_streaming: true,
            ..Default::default()
        };

        let result = router.resolve_target("unknown/model", Some(&reqs), Some(&registry));
        assert!(!result.is_empty());
        assert_eq!(result[0].name, "fallback-model");
    }
}
