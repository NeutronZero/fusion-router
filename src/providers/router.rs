use async_trait::async_trait;
use std::sync::Arc;

use super::circuit_breaking_provider::CircuitBreakingProvider;
use super::{ChatProvider, Provider};
use crate::types::ChatCompletionRequest;

pub struct ProviderRouter {
    providers: Vec<(Vec<String>, Arc<CircuitBreakingProvider>)>,
    default: Arc<CircuitBreakingProvider>,
}

impl ProviderRouter {
    pub fn new(default: Arc<Provider>) -> Self {
        let name = default.name().to_string();
        Self {
            providers: Vec::new(),
            default: Arc::new(CircuitBreakingProvider::new(default, 5, 3, 30, name)),
        }
    }

    pub fn with_provider(
        mut self,
        model_prefixes: Vec<String>,
        provider: Arc<Provider>,
    ) -> Self {
        let name = provider.name().to_string();
        self.providers.push((
            model_prefixes,
            Arc::new(CircuitBreakingProvider::new(provider, 5, 3, 30, name)),
        ));
        self
    }

    fn matching_providers(&self, model: &str) -> Vec<&Arc<CircuitBreakingProvider>> {
        let mut matched = Vec::new();
        for (prefixes, provider) in &self.providers {
            for prefix in prefixes {
                if model.starts_with(prefix) {
                    matched.push(provider);
                    break;
                }
            }
        }
        matched
    }
}

#[async_trait]
impl ChatProvider for ProviderRouter {
    fn name(&self) -> &str {
        "router"
    }

    async fn chat_completion(&self, request: &ChatCompletionRequest) -> anyhow::Result<crate::types::ChatCompletionResponse> {
        let matched = self.matching_providers(&request.model);
        let providers_to_try: Vec<&Arc<CircuitBreakingProvider>> = if matched.is_empty() {
            vec![&self.default]
        } else {
            matched
        };

        let mut last_error: Option<anyhow::Error> = None;

        for provider in providers_to_try {
            if !provider.breaker().can_execute() {
                tracing::warn!(
                    provider = %provider.name(),
                    "circuit is open, skipping"
                );
                continue;
            }

            tracing::debug!(model = %request.model, target = %provider.name(), "routing request");

            match provider.chat_completion(request).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    tracing::warn!(
                        provider = %provider.name(),
                        error = %e,
                        "provider failed, trying next"
                    );
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no available providers")))
    }
}
