use async_trait::async_trait;
use futures::stream::BoxStream;
use std::collections::HashMap;
use std::sync::Arc;

use super::{ChatProvider, Provider};
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ChatStreamChunk};

/// A `ChatProvider` decorator that injects configured custom headers into
/// every outgoing `TransportRequest`.
///
/// Merge semantics: explicitly configured headers WIN over headers set by the
/// inner provider's model (including `Authorization`, when the operator
/// explicitly configures it). Keys that are not configured leave the inner
/// provider's own headers untouched.
pub struct ProviderWithHeaders {
    inner: Arc<Provider>,
    headers: HashMap<String, String>,
}

impl ProviderWithHeaders {
    pub fn new(inner: Arc<Provider>, headers: HashMap<String, String>) -> Self {
        inner.set_extra_headers(headers.clone());
        Self { inner, headers }
    }

    /// The headers this decorator injects (for diagnostics/tests).
    pub fn configured_headers(&self) -> &HashMap<String, String> {
        &self.headers
    }
}

#[async_trait]
impl ChatProvider for ProviderWithHeaders {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<ChatCompletionResponse> {
        self.inner.chat_completion(request).await
    }

    async fn chat_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<ChatStreamChunk>>> {
        self.inner.chat_stream(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Model;
    use crate::providers::ModelCapabilities;
    use crate::providers::ModelPricing;
    use crate::transport::{
        Transport, TransportError, TransportEvent, TransportRequest, TransportResponse,
    };
    use crate::types::{ChatMessage, Choice, NanoUSD};
    use async_trait::async_trait;
    use parking_lot::Mutex;

    // -- capturing test doubles ----------------------------------------------

    #[derive(Debug, Clone, Default)]
    struct CapturingModel;
    #[async_trait]
    impl Model for CapturingModel {
        fn id(&self) -> &str {
            "capture"
        }
        fn provider_name(&self) -> &str {
            "capture"
        }
        fn capabilities(&self) -> ModelCapabilities {
            Default::default()
        }
        fn pricing(&self) -> ModelPricing {
            ModelPricing {
                input_cost_per_1k: NanoUSD::ZERO,
                output_cost_per_1k: NanoUSD::ZERO,
            }
        }
        fn quota_remaining(&self) -> Option<f64> {
            None
        }

        fn format_request(
            &self,
            _req: &ChatCompletionRequest,
            api_key: &str,
        ) -> anyhow::Result<TransportRequest> {
            let mut headers = HashMap::new();
            // The model sets Authorization itself from the resolved key.
            headers.insert("Authorization".to_string(), format!("Bearer {api_key}"));
            headers.insert("Content-Type".to_string(), "application/json".to_string());
            Ok(TransportRequest {
                url: "http://localhost/v1/chat/completions".into(),
                method: "POST".into(),
                headers,
                body: serde_json::json!({}),
            })
        }

        fn normalize_response(
            &self,
            _resp: TransportResponse,
        ) -> anyhow::Result<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                id: "r".into(),
                object: "chat.completion".into(),
                created: 0,
                model: "capture".into(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "ok".into(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: None,
            })
        }
    }

    #[derive(Clone, Default)]
    struct SharedCapture(Arc<Mutex<Vec<TransportRequest>>>);

    impl SharedCapture {
        fn last_request(&self) -> TransportRequest {
            self.0
                .lock()
                .last()
                .cloned()
                .expect("a request must have been sent")
        }
    }

    struct CaptureTransport {
        seen: SharedCapture,
    }
    #[async_trait]
    impl Transport for CaptureTransport {
        async fn send(&self, req: TransportRequest) -> Result<TransportResponse, TransportError> {
            self.seen.0.lock().push(req);
            Ok(TransportResponse {
                status: 200,
                body: serde_json::json!({}),
            })
        }
        async fn stream(
            &self,
            _req: TransportRequest,
        ) -> Result<
            futures::stream::BoxStream<'static, Result<TransportEvent, TransportError>>,
            TransportError,
        > {
            unreachable!("stream not exercised by these tests")
        }
    }

    fn capture_provider() -> (Arc<Provider>, SharedCapture) {
        let capture = SharedCapture::default();
        let provider = Arc::new(Provider::new(
            Box::new(CapturingModel),
            Box::new(CaptureTransport {
                seen: capture.clone(),
            }),
            "sk-inner-key".to_string(),
        ));
        (provider, capture)
    }

    async fn send_once(provider: &Arc<Provider>) {
        let req = ChatCompletionRequest {
            model: "m".into(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            files: None,
            execution: None,
            output: None,
            strategy: None,
        };
        provider.chat_completion(&req).await.unwrap();
    }

    // -- assertions -----------------------------------------------------------

    #[tokio::test]
    async fn test_configured_header_appears_on_built_request() {
        let (inner, capture) = capture_provider();
        let mut headers = HashMap::new();
        headers.insert("X-Custom-Header".to_string(), "test-value".to_string());
        let decorator = ProviderWithHeaders::new(inner.clone(), headers);

        assert_eq!(decorator.name(), "capture");
        assert_eq!(
            decorator.configured_headers().get("X-Custom-Header"),
            Some(&"test-value".to_string())
        );

        send_once(&inner).await;
        let sent = capture.last_request();
        assert_eq!(
            sent.headers.get("X-Custom-Header").map(String::as_str),
            Some("test-value"),
            "configured custom header must be present on the built TransportRequest"
        );
    }

    #[tokio::test]
    async fn test_inner_authorization_preserved_when_not_configured() {
        let (inner, capture) = capture_provider();
        let mut headers = HashMap::new();
        headers.insert("X-Trace-Id".to_string(), "t-1".to_string());
        let _decorator = ProviderWithHeaders::new(inner.clone(), headers);

        send_once(&inner).await;
        let sent = capture.last_request();
        assert_eq!(
            sent.headers.get("Authorization").map(String::as_str),
            Some("Bearer sk-inner-key"),
            "inner-set Authorization must survive when custom headers don't configure it"
        );
        assert_eq!(
            sent.headers.get("X-Trace-Id").map(String::as_str),
            Some("t-1")
        );
    }

    #[tokio::test]
    async fn test_explicitly_configured_authorization_wins() {
        let (inner, capture) = capture_provider();
        let mut headers = HashMap::new();
        headers.insert(
            "Authorization".to_string(),
            "Bearer operator-configured".to_string(),
        );
        let _decorator = ProviderWithHeaders::new(inner.clone(), headers);

        send_once(&inner).await;
        let sent = capture.last_request();
        assert_eq!(
            sent.headers.get("Authorization").map(String::as_str),
            Some("Bearer operator-configured"),
            "explicit configuration wins over the inner provider's Authorization"
        );
    }
}
