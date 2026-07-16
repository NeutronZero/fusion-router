use crate::transport::HttpTransport;
use super::zen_model::ZenModel;
use super::Provider;
use std::time::Duration;

pub fn new_zen_provider(api_key: String) -> Provider {
    let model = Box::new(ZenModel::new("opencode-zen-model".to_string()));
    let transport = Box::new(HttpTransport::new(Duration::from_secs(30)));
    Provider::new(model, transport, api_key)
}

pub struct ZenProvider;

impl ZenProvider {
    pub fn new(api_key: String) -> Provider {
        new_zen_provider(api_key)
    }
}
