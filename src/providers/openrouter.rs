use super::openrouter_model::OpenRouterModel;
use super::Provider;
use crate::transport::HttpTransport;
use std::time::Duration;

pub fn new_openrouter_provider(api_key: String) -> Provider {
    new_openrouter_provider_with_base_url(api_key, None)
}

pub fn new_openrouter_provider_with_base_url(
    api_key: String,
    base_url: Option<String>,
) -> Provider {
    let model = Box::new(OpenRouterModel::with_base_url(
        "openrouter-model".to_string(),
        base_url,
    ));
    let transport = Box::new(
        HttpTransport::new(Duration::from_secs(600))
            .expect("failed to build OpenRouter HTTP transport (hardened client required)"),
    );
    Provider::new(model, transport, api_key)
}

pub struct OpenRouterProvider;

impl OpenRouterProvider {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(api_key: String) -> Provider {
        new_openrouter_provider(api_key)
    }

    /// Configured base URL wins over the `OPENROUTER_BASE_URL` env default.
    #[allow(clippy::new_ret_no_self)]
    pub fn with_base_url(api_key: String, base_url: Option<String>) -> Provider {
        new_openrouter_provider_with_base_url(api_key, base_url)
    }
}
