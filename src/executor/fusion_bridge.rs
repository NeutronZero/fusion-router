//! Bridge between the src provider interfaces and `fusion_runtime`
//! (Phase 6.4 executor delegation).
//!
//! - `FusionChatProvider` adapts `crate::providers::ChatProvider` to the
//!   `fusion_runtime::ChatProvider` contract, preserving the failure wording
//!   (`"Provider error: …"`) the src executor has always surfaced.
//!
//! The bridge is cheap: no provider state, only `Arc` clones, so it may be
//! rebuilt per node execution without meaningful overhead.

use std::sync::Arc;

use async_trait::async_trait;

use crate::providers::ChatProvider;
use crate::types::ChatCompletionRequest;

/// Adapts a src `ChatProvider` to the `fusion_runtime::ChatProvider` contract.
pub struct FusionChatProvider {
    inner: Arc<dyn ChatProvider + Send + Sync>,
    #[cfg(feature = "semantic-cache")]
    cache: Option<Arc<crate::cache::SemanticCache>>,
    /// Cache isolation namespace (review M4). Entries are keyed under this
    /// namespace so deployments serving multiple trust domains can scope
    /// caches per tenant; the default "global" is only appropriate for
    /// single-tenant operation.
    #[cfg(feature = "semantic-cache")]
    cache_namespace: String,
}

impl FusionChatProvider {
    pub fn new(inner: Arc<dyn ChatProvider + Send + Sync>) -> Self {
        Self {
            inner,
            #[cfg(feature = "semantic-cache")]
            cache: None,
            #[cfg(feature = "semantic-cache")]
            cache_namespace: "global".to_string(),
        }
    }

    #[cfg(feature = "semantic-cache")]
    pub fn with_cache(mut self, cache: Option<Arc<crate::cache::SemanticCache>>) -> Self {
        self.cache = cache;
        self
    }

    /// Scopes all cache lookups/writes to this namespace (review M4).
    #[cfg(feature = "semantic-cache")]
    pub fn with_cache_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.cache_namespace = namespace.into();
        self
    }

    #[cfg(feature = "semantic-cache")]
    fn cache_key(
        namespace: &str,
        model: &str,
        messages: &[fusion_runtime::ChatMessage],
        temperature: Option<f32>,
        max_tokens: Option<u32>,
    ) -> String {
        use sha2::{Digest, Sha256};

        // Sampling params change outputs, so they are part of the key
        // material. Hash the canonical JSON to keep keys bounded.
        let canonical = serde_json::json!({
            "namespace": namespace,
            "model": model,
            "messages": messages,
            "temperature": temperature,
            "max_tokens": max_tokens,
        });
        let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
        let digest = Sha256::digest(&bytes);
        let mut hex = String::with_capacity(64);
        for b in digest {
            hex.push_str(&format!("{b:02x}"));
        }
        hex
    }

    fn to_src_request(request: &fusion_runtime::ChatRequest) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: request.model.clone(),
            messages: request
                .messages
                .iter()
                .map(|m| crate::types::ChatMessage {
                    role: m.role.clone(),
                    content: m.content.clone(),
                })
                .collect(),
            stream: false,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            tools: None,
            files: None,
            execution: None,
            output: None,
            strategy: None,
        }
    }

    fn to_runtime_response(
        response: &crate::types::ChatCompletionResponse,
    ) -> fusion_runtime::ChatResponse {
        let content = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        fusion_runtime::ChatResponse {
            content,
            usage: response.usage.clone(),
            tool_calls: response.native_tool_calls.clone().unwrap_or_default(),
            tool_results: Vec::new(),
        }
    }
}

#[async_trait]
impl fusion_runtime::ChatProvider for FusionChatProvider {
    async fn chat_completion(
        &self,
        request: &fusion_runtime::ChatRequest,
    ) -> Result<fusion_runtime::ChatResponse, String> {
        #[cfg(feature = "semantic-cache")]
        if let Some(ref cache) = self.cache {
            let key = Self::cache_key(
                &self.cache_namespace,
                &request.model,
                &request.messages,
                request.temperature,
                request.max_tokens,
            );
            if let Some(hit) = cache.lookup(&key).await {
                if let Some(content) = hit
                    .response
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                {
                    // Usage recorded with the response must round-trip on a
                    // hit so token telemetry is not silently dropped.
                    let usage = hit
                        .usage
                        .and_then(|u| serde_json::from_value::<crate::types::Usage>(u).ok());
                    return Ok(fusion_runtime::ChatResponse {
                        content,
                        usage,
                        tool_calls: Vec::new(),
                        tool_results: Vec::new(),
                    });
                }
            }
        }

        let src_request = Self::to_src_request(request);
        match self.inner.chat_completion(&src_request).await {
            Ok(response) => {
                #[cfg(feature = "semantic-cache")]
                if let Some(ref cache) = self.cache {
                    let content = response
                        .choices
                        .first()
                        .map(|c| c.message.content.clone())
                        .unwrap_or_default();
                    if !content.trim().is_empty() {
                        let key = Self::cache_key(
                            &self.cache_namespace,
                            &request.model,
                            &request.messages,
                            request.temperature,
                            request.max_tokens,
                        );
                        let usage = response
                            .usage
                            .as_ref()
                            .and_then(|u| serde_json::to_value(u).ok());
                        cache
                            .put_with_usage(&key, serde_json::json!({ "content": content }), usage)
                            .await;
                    }
                }
                Ok(Self::to_runtime_response(&response))
            }
            Err(e) => Err(format!("Provider error: {e}")),
        }
    }
}

/// Re-exports the `fusion_runtime::ChatProvider` trait alias so callers do
/// not need to spell the fully-qualified path twice.
pub use fusion_runtime::ChatProvider as FusionRuntimeChatProvider;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    struct CountingProvider(Arc<std::sync::atomic::AtomicUsize>);

    #[async_trait]
    impl ChatProvider for CountingProvider {
        fn name(&self) -> &str {
            "counting"
        }

        async fn chat_completion(
            &self,
            request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ChatCompletionResponse {
                id: "c".into(),
                object: "chat.completion".into(),
                created: 0,
                model: request.model.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "bridge response".into(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: Some(Usage {
                    prompt_tokens: 7,
                    completion_tokens: 3,
                    total_tokens: 10,
                }),
            })
        }
    }

    #[tokio::test]
    async fn maps_request_and_response() {
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let bridge = FusionChatProvider::new(Arc::new(CountingProvider(counter.clone())));
        let response = bridge
            .chat_completion(&fusion_runtime::ChatRequest {
                model: "gpt-4".into(),
                messages: vec![fusion_runtime::ChatMessage {
                    role: "user".into(),
                    content: "hi".into(),
                }],
                temperature: Some(0.5),
                max_tokens: Some(64),
            })
            .await
            .expect("chat");
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(response.content, "bridge response");
        assert_eq!(response.usage.unwrap().total_tokens, 10);
        assert!(response.tool_calls.is_empty());
    }

    #[tokio::test]
    async fn provider_error_keeps_src_wording() {
        struct Failing;
        #[async_trait]
        impl ChatProvider for Failing {
            fn name(&self) -> &str {
                "failing"
            }
            async fn chat_completion(
                &self,
                _request: &ChatCompletionRequest,
            ) -> anyhow::Result<ChatCompletionResponse> {
                anyhow::bail!("provider exploded")
            }
        }

        let bridge = FusionChatProvider::new(Arc::new(Failing));
        let err = bridge
            .chat_completion(&fusion_runtime::ChatRequest {
                model: "m".into(),
                messages: vec![],
                temperature: None,
                max_tokens: None,
            })
            .await
            .expect_err("must fail");
        assert_eq!(err, "Provider error: provider exploded");
    }

    #[cfg(feature = "semantic-cache")]
    #[test]
    fn cache_key_differs_when_temperature_differs() {
        let messages = vec![fusion_runtime::ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let cold = FusionChatProvider::cache_key("ns", "m", &messages, Some(0.0), Some(64));
        let warm = FusionChatProvider::cache_key("ns", "m", &messages, Some(0.7), Some(64));
        assert_ne!(cold, warm, "temperature must be part of the cache key");
    }

    #[cfg(feature = "semantic-cache")]
    #[test]
    fn cache_key_differs_when_max_tokens_differs() {
        let messages = vec![fusion_runtime::ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let short = FusionChatProvider::cache_key("ns", "m", &messages, Some(0.5), Some(16));
        let long = FusionChatProvider::cache_key("ns", "m", &messages, Some(0.5), Some(4096));
        assert_ne!(short, long, "max_tokens must be part of the cache key");
    }

    #[cfg(feature = "semantic-cache")]
    #[test]
    fn cache_key_is_stable_and_bounded() {
        let messages = vec![fusion_runtime::ChatMessage {
            role: "user".into(),
            content: "deterministic".into(),
        }];
        let a = FusionChatProvider::cache_key("ns", "m", &messages, None, None);
        let b = FusionChatProvider::cache_key("ns", "m", &messages, None, None);
        assert_eq!(a, b, "same inputs must produce identical keys");
        assert_eq!(a.len(), 64, "key is a hex sha256 digest");
    }

    #[cfg(feature = "semantic-cache")]
    #[tokio::test]
    async fn cached_hit_round_trips_usage() {
        use crate::cache::{embeddings::MockEmbedder as BridgeMockEmbedder, SemanticCache};

        struct Recording;
        #[async_trait]
        impl ChatProvider for Recording {
            fn name(&self) -> &str {
                "recording"
            }
            async fn chat_completion(
                &self,
                _request: &ChatCompletionRequest,
            ) -> anyhow::Result<ChatCompletionResponse> {
                Ok(ChatCompletionResponse {
                    id: "r".into(),
                    object: "chat.completion".into(),
                    created: 0,
                    model: "m".into(),
                    choices: vec![Choice {
                        index: 0,
                        message: ChatMessage {
                            role: "assistant".into(),
                            content: "cached!".into(),
                        },
                        finish_reason: "stop".into(),
                    }],
                    native_tool_calls: None,
                    usage: Some(Usage {
                        prompt_tokens: 5,
                        completion_tokens: 2,
                        total_tokens: 7,
                    }),
                })
            }
        }

        let cache = Arc::new(
            SemanticCache::new(Arc::new(BridgeMockEmbedder), 0.9, 100, 384).expect("cache init"),
        );
        let bridge = FusionChatProvider::new(Arc::new(Recording)).with_cache(Some(cache.clone()));

        let request = fusion_runtime::ChatRequest {
            model: "m".into(),
            messages: vec![fusion_runtime::ChatMessage {
                role: "user".into(),
                content: "q".into(),
            }],
            temperature: Some(0.1),
            max_tokens: Some(32),
        };

        let first = bridge.chat_completion(&request).await.expect("first call");
        assert_eq!(first.usage.unwrap().total_tokens, 7);

        // Second call must hit the cache and still surface usage telemetry.
        let second = bridge.chat_completion(&request).await.expect("cached call");
        assert_eq!(second.content, "cached!");
        let usage = second.usage.expect("usage must survive a cache hit");
        assert_eq!(
            (
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.total_tokens
            ),
            (5, 2, 7)
        );
    }
}
