use crate::transport::HttpTransport;
use super::ollama_model::OllamaModel;
use super::Provider;
use std::time::Duration;

pub fn new_ollama_provider() -> Provider {
    let model = Box::new(OllamaModel::new("ollama-model".to_string()));
    let transport = Box::new(HttpTransport::new(Duration::from_secs(30)));
    Provider::new(model, transport, String::new())
}

pub struct OllamaProvider;

impl OllamaProvider {
    pub fn new() -> Provider {
        new_ollama_provider()
    }
}
