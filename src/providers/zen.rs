use crate::transport::HttpTransport;
use super::zen_model::ZenModel;
use super::Provider;
use std::time::Duration;

pub fn new_zen_provider(api_key: String) -> Provider {
    new_zen_provider_with_base_url(api_key, None)
}

pub fn new_zen_provider_with_base_url(
    api_key: String,
    base_url: Option<String>,
) -> Provider {
    let model = Box::new(ZenModel::with_base_url("opencode-zen-model".to_string(), base_url));
    let transport = Box::new(
        HttpTransport::new(Duration::from_secs(300))
            .expect("failed to build HTTP transport for zen provider"),
    );
    Provider::new(model, transport, api_key)
}

pub struct ZenProvider;

impl ZenProvider {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(api_key: String) -> Provider {
        new_zen_provider(api_key)
    }

    /// Configured base URL wins over the `OPENCODEZEN_BASE_URL` env default.
    #[allow(clippy::new_ret_no_self)]
    pub fn with_base_url(api_key: String, base_url: Option<String>) -> Provider {
        new_zen_provider_with_base_url(api_key, base_url)
    }
}
