use crate::transport::HttpTransport;
use super::openrouter_model::OpenRouterModel;
use super::Provider;
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
            .unwrap_or_default(),
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
