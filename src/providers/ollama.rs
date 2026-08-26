use super::ollama_model::OllamaModel;
use super::Provider;
use crate::transport::HttpTransport;
use std::time::Duration;

pub fn new_ollama_provider() -> Provider {
    let model = Box::new(OllamaModel::new("ollama-model".to_string()));
    // 30s total timeout retained: local inference is latency-bounded and the
    // model layer does not expose a streaming/non-streaming transport split,
    // so a per-phase split is not trivially separable here.
    let transport = Box::new(
        HttpTransport::new(Duration::from_secs(30))
            .expect("failed to build Ollama HTTP transport (hardened client required)"),
    );
    Provider::new(model, transport, String::new())
}

pub struct OllamaProvider;

impl OllamaProvider {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> Provider {
        new_ollama_provider()
    }
}
