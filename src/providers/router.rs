use async_trait::async_trait;
use std::sync::Arc;

use super::{ChatProvider, Provider};
use crate::types::ChatCompletionRequest;

pub struct ProviderRouter {
    providers: Vec<(Vec<String>, Arc<Provider>)>,
    default: Arc<Provider>,
}

impl ProviderRouter {
    pub fn new(default: Arc<Provider>) -> Self {
        Self {
            providers: Vec::new(),
            default,
        }
    }

    pub fn with_provider(
        mut self,
        model_prefixes: Vec<String>,
        provider: Arc<Provider>,
    ) -> Self {
        self.providers.push((model_prefixes, provider));
        self
    }

    fn resolve(&self, model: &str) -> &Arc<Provider> {
        for (prefixes, provider) in &self.providers {
            for prefix in prefixes {
                if model.starts_with(prefix) {
                    return provider;
                }
            }
        }
        &self.default
    }
}

#[async_trait]
impl ChatProvider for ProviderRouter {
    fn name(&self) -> &str {
        "router"
    }

    async fn chat_completion(&self, request: &ChatCompletionRequest) -> anyhow::Result<crate::types::ChatCompletionResponse> {
        let provider = self.resolve(&request.model);
        tracing::debug!(model = %request.model, target = %provider.name(), "routing request");
        provider.chat_completion(request).await
    }
}
