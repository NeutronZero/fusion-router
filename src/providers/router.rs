use async_trait::async_trait;
use std::sync::Arc;

use super::Provider;
use crate::types::ChatCompletionRequest;

pub struct ProviderRouter {
    providers: Vec<(Vec<String>, Arc<dyn Provider + Send + Sync>)>,
    default: Arc<dyn Provider + Send + Sync>,
}

impl ProviderRouter {
    pub fn new(default: Arc<dyn Provider + Send + Sync>) -> Self {
        Self {
            providers: Vec::new(),
            default,
        }
    }

    pub fn with_provider(
        mut self,
        model_prefixes: Vec<String>,
        provider: Arc<dyn Provider + Send + Sync>,
    ) -> Self {
        self.providers.push((model_prefixes, provider));
        self
    }

    fn resolve(&self, model: &str) -> &Arc<dyn Provider + Send + Sync> {
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
impl Provider for ProviderRouter {
    fn name(&self) -> &str {
        "router"
    }

    async fn chat_completion(&self, request: &ChatCompletionRequest) -> anyhow::Result<crate::types::ChatCompletionResponse> {
        let provider = self.resolve(&request.model);
        tracing::debug!(model = %request.model, target = %provider.name(), "routing request");
        provider.chat_completion(request).await
    }
}
