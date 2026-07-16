use crate::transport::HttpTransport;
use super::openrouter_model::OpenRouterModel;
use super::Provider;
use std::time::Duration;

pub fn new_openrouter_provider(api_key: String) -> Provider {
    let model = Box::new(OpenRouterModel::new("openrouter-model".to_string()));
    let transport = Box::new(HttpTransport::new(Duration::from_secs(30)));
    Provider::new(model, transport, api_key)
}

pub struct OpenRouterProvider;

impl OpenRouterProvider {
    pub fn new(api_key: String) -> Provider {
        new_openrouter_provider(api_key)
    }
}
