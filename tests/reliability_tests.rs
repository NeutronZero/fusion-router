use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use async_trait::async_trait;
use fusion_router::providers::circuit_breaker::{CircuitBreaker, CircuitState};
use fusion_router::providers::router::{ProviderRouter, ProviderTarget};
use fusion_router::providers::ChatProvider;
use fusion_router::types::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice};

/// A provider that succeeds for a fixed number of calls, then returns errors.
/// Simulates a provider that goes down after an initial healthy period.
struct FailingProvider {
    call_count: AtomicU64,
    success_before_fail: u64,
    name: String,
}

impl FailingProvider {
    fn new(name: &str, success_before_fail: u64) -> Self {
        Self {
            call_count: AtomicU64::new(0),
            success_before_fail,
            name: name.to_string(),
        }
    }
}

#[async_trait]
impl ChatProvider for FailingProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat_completion(
        &self,
        _request: &ChatCompletionRequest,
    ) -> anyhow::Result<ChatCompletionResponse> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);
        if count < self.success_before_fail {
            Ok(ChatCompletionResponse {
                id: "test".into(),
                object: "chat.completion".into(),
                created: 0,
                model: self.name.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "ok".into(),
                    },
                    finish_reason: "stop".into(),
                }],
                usage: None,
            })
        } else {
            Err(anyhow::anyhow!("{} provider outage simulated", self.name))
        }
    }
}

/// A provider that always succeeds and tracks call count.
struct AlwaysOkProvider {
    name: String,
    call_count: AtomicU64,
}

impl AlwaysOkProvider {
    fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            call_count: AtomicU64::new(0),
        }
    }
}

#[async_trait]
impl ChatProvider for AlwaysOkProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn chat_completion(
        &self,
        _request: &ChatCompletionRequest,
    ) -> anyhow::Result<ChatCompletionResponse> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(ChatCompletionResponse {
            id: "test".into(),
            object: "chat.completion".into(),
            created: 0,
            model: self.name.clone(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: "ok".into(),
                },
                finish_reason: "stop".into(),
            }],
            usage: None,
        })
    }
}

fn test_request(model: &str) -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: model.into(),
        messages: vec![],
        stream: false,
        temperature: None,
        max_tokens: None,
        tools: None,
        files: None,
        execution: None,
        output: None,
    }
}

fn make_target(
    name: &str,
    provider: Arc<dyn ChatProvider + Send + Sync>,
    breaker: CircuitBreaker,
) -> ProviderTarget {
    ProviderTarget::new(
        name.into(),
        breaker,
        Box::new(move || provider.clone()),
    )
}

#[tokio::test]
async fn test_provider_outage_simulation() {
    // Phase 1: Set up providers
    let failing = Arc::new(FailingProvider::new("primary", 2));
    let fallback = Arc::new(AlwaysOkProvider::new("fallback"));

    // Phase 2: Create circuit breaker and verify initial Closed state
    let breaker = CircuitBreaker::new(3, 1, 60);
    assert_eq!(breaker.state(), CircuitState::Closed, "breaker should start Closed");
    assert!(breaker.can_execute(), "breaker should allow execution initially");

    let primary_target = make_target("primary", failing.clone(), breaker);
    let fallback_target = make_target(
        "fallback",
        fallback.clone(),
        CircuitBreaker::new(3, 1, 60),
    );
    let default_target = make_target(
        "default",
        Arc::new(AlwaysOkProvider::new("default")),
        CircuitBreaker::new(3, 1, 60),
    );

    // Phase 3: Route with primary (first match) then fallback (second match)
    let router = ProviderRouter::new(default_target)
        .with_provider(vec!["test/".into()], primary_target)
        .with_provider(vec!["test/".into()], fallback_target);

    let req = test_request("test/model");

    // Phase 4: Primary healthy — handles requests directly
    for i in 0..2 {
        let resp = router.chat_completion(&req).await.unwrap();
        assert_eq!(
            resp.model, "primary",
            "request {}: primary should handle while healthy",
            i + 1
        );
    }

    // Phase 5: Primary starts failing — fallback handles.
    // After 3 failures the circuit opens (requests 2-4 = indices 2,3,4).
    for i in 2..5 {
        let resp = router.chat_completion(&req).await.unwrap();
        assert_eq!(
            resp.model, "fallback",
            "request {}: fallback should handle during outage",
            i + 1
        );
    }

    // Phase 6: Circuit is now Open — primary skipped, fallback handles directly
    for i in 5..8 {
        let resp = router.chat_completion(&req).await.unwrap();
        assert_eq!(
            resp.model, "fallback",
            "request {}: fallback should handle after circuit open",
            i + 1
        );
    }

    // Phase 7: Verify exact call counts proving circuit state transitions
    // Primary: 2 success + 3 failures = 5 calls (no more after circuit opens)
    assert_eq!(
        failing.call_count.load(Ordering::Relaxed),
        5,
        "primary called 2x (success) + 3x (failure) before circuit opened"
    );

    // Fallback: requests 3-8 = 6 calls (all after primary started failing)
    assert_eq!(
        fallback.call_count.load(Ordering::Relaxed),
        6,
        "fallback handled all 6 requests after primary failure"
    );
}
