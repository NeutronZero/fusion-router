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

/// A provider that succeeds for N calls, fails for M calls, then recovers indefinitely.
struct RecoveringProvider {
    call_count: AtomicU64,
    success_before_fail: u64,
    fail_count: u64,
    name: String,
}

impl RecoveringProvider {
    fn new(name: &str, success_before_fail: u64, fail_count: u64) -> Self {
        Self {
            call_count: AtomicU64::new(0),
            success_before_fail,
            fail_count,
            name: name.into(),
        }
    }
}

#[async_trait]
impl ChatProvider for RecoveringProvider {
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
        } else if count < self.success_before_fail + self.fail_count {
            Err(anyhow::anyhow!("{} provider outage simulated", self.name))
        } else {
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

#[tokio::test]
async fn test_state_convergence_after_outage() {
    let primary_provider = Arc::new(RecoveringProvider::new("primary", 2, 6));
    let fallback = Arc::new(AlwaysOkProvider::new("fallback"));

    let breaker = CircuitBreaker::new(3, 2, 0);
    assert_eq!(breaker.state(), CircuitState::Closed);

    let primary_target = make_target("primary", primary_provider.clone(), breaker);
    let fallback_target = make_target(
        "fallback",
        fallback.clone(),
        CircuitBreaker::new(3, 2, 0),
    );
    let default_target = make_target(
        "default",
        Arc::new(AlwaysOkProvider::new("default")),
        CircuitBreaker::new(3, 2, 0),
    );

    let router = ProviderRouter::new(default_target)
        .with_provider(vec!["test/".into()], primary_target)
        .with_provider(vec!["test/".into()], fallback_target);

    let req = test_request("test/model");

    // Phase 1: Closed — primary handles both requests
    for i in 0..2 {
        let resp = router.chat_completion(&req).await.unwrap();
        assert_eq!(resp.model, "primary", "req {}: primary handles in closed state", i + 1);
    }

    // Phase 2: Failure threshold reached — circuit opens after 3rd failure.
    // Reqs 3-4: primary fails, fallback takes over (Closed, fail_count=1,2).
    // Req 5: primary fails → failure_count=3 → Open. Fallback handles.
    for i in 2..5 {
        let resp = router.chat_completion(&req).await.unwrap();
        assert_eq!(resp.model, "fallback", "req {}: fallback handles as circuit opens", i + 1);
    }

    // Phase 3: Circuit Open — primary skipped, fallback handles.
    // With cooldown=0, can_execute immediately transitions Open→HalfOpen
    // each time, but the provider is still failing so HalfOpen→Open again.
    // Reqs 6-8: oscillating Open↔HalfOpen while failing, fallback handles.
    for i in 5..8 {
        let resp = router.chat_completion(&req).await.unwrap();
        assert_eq!(resp.model, "fallback", "req {}: fallback during open/half-open oscillation", i + 1);
    }

    // Phase 4: Provider recovers — converges Closed→Open→HalfOpen→Closed.
    // Req 9: can_execute→HalfOpen, primary succeeds → HalfOpen (success_count=1).
    assert_eq!(
        router.chat_completion(&req).await.unwrap().model,
        "primary",
        "req 9: first recovered request transitions to half-open"
    );

    // Req 10: HalfOpen, primary succeeds → Closed (success_count=2 ≥ 2).
    assert_eq!(
        router.chat_completion(&req).await.unwrap().model,
        "primary",
        "req 10: second success closes the circuit"
    );

    // Phase 5: Fully recovered — primary handles in Closed state.
    for i in 11..14 {
        let resp = router.chat_completion(&req).await.unwrap();
        assert_eq!(resp.model, "primary", "req {}: primary handles after full recovery", i);
    }

    // Call count verification:
    // Primary: 2 success + 6 failure + 2 success (half-open recovery) + 3 (closed) = 13
    assert_eq!(
        primary_provider.call_count.load(Ordering::Relaxed),
        13,
        "primary called 13 times total across all phases"
    );
    // Fallback: requests 3-8 = 6 calls
    assert_eq!(
        fallback.call_count.load(Ordering::Relaxed),
        6,
        "fallback handled requests during outage only"
    );
}
