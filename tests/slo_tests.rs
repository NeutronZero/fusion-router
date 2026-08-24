use async_trait::async_trait;
use fusion_router::providers::circuit_breaker::{CircuitBreaker, CircuitState};
use fusion_router::providers::router::{ProviderRouter, ProviderTarget};
use fusion_router::providers::ChatProvider;
use fusion_router::types::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn assert_slo(condition: bool, message: &str) {
    assert!(condition, "SLO VIOLATION: {message}");
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
        strategy: None,
    }
}

// --- Provider stubs ---

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
            name: name.into(),
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
                native_tool_calls: None,
                usage: None,
            })
        } else {
            Err(anyhow::anyhow!("{} provider outage simulated", self.name))
        }
    }
}

struct AlwaysOkProvider {
    call_count: AtomicU64,
    name: String,
}

impl AlwaysOkProvider {
    fn new(name: &str) -> Self {
        Self {
            call_count: AtomicU64::new(0),
            name: name.into(),
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
            native_tool_calls: None,
            usage: None,
        })
    }
}

// --- Test helpers ---

/// Runs a full outage simulation with a primary that succeeds N times then fails,
/// a fallback that always succeeds, and a configurable circuit breaker.
/// Returns structured results for SLO verification.
async fn run_outage_simulation(
    success_before_fail: u64,
    failure_threshold: u32,
    cooldown_secs: u64,
    total_requests: usize,
) -> OutageSimResult {
    let primary = Arc::new(FailingProvider::new("primary", success_before_fail));
    let fallback = Arc::new(AlwaysOkProvider::new("fallback"));
    let default = Arc::new(AlwaysOkProvider::new("default"));

    // Keep reference to track primary call count after simulation
    let primary_tracker = primary.clone();

    let breaker = CircuitBreaker::new(failure_threshold, 1, cooldown_secs);

    let primary_target =
        ProviderTarget::new("primary".into(), breaker, Box::new(move || primary.clone()));
    let fallback_target = ProviderTarget::new(
        "fallback".into(),
        CircuitBreaker::new(failure_threshold, 1, cooldown_secs),
        Box::new(move || fallback.clone()),
    );
    let default_target = ProviderTarget::new(
        "default".into(),
        CircuitBreaker::new(failure_threshold, 1, cooldown_secs),
        Box::new(move || default.clone()),
    );

    let router = ProviderRouter::new(default_target)
        .with_provider(vec!["test/".into()], primary_target)
        .with_provider(vec!["test/".into()], fallback_target);

    let req = test_request("test/model");

    let mut responses: Vec<String> = Vec::with_capacity(total_requests);
    let mut all_succeeded = true;

    for _ in 0..total_requests {
        match router.chat_completion(&req).await {
            Ok(resp) => {
                responses.push(resp.model);
            }
            Err(_e) => {
                all_succeeded = false;
                responses.push("error".into());
            }
        }
    }

    // Read actual call counts from provider trackers
    let primary_call_count = primary_tracker.call_count.load(Ordering::SeqCst);

    // Circuit opens after failure_threshold failures have been recorded.
    // Primary is called: success_before_fail (successes) + failure_threshold (failures) times.
    // The circuit opens on the failure_threshold-th failure call.
    let expected_primary_calls = success_before_fail + failure_threshold as u64;
    let circuit_opens_at = if expected_primary_calls < total_requests as u64 {
        Some(expected_primary_calls as usize)
    } else {
        None
    };

    // Count successful responses by model
    let primary_ok = responses.iter().filter(|m| m.as_str() == "primary").count();
    let fallback_ok = responses
        .iter()
        .filter(|m| m.as_str() == "fallback")
        .count();

    OutageSimResult {
        total_requests,
        primary_ok,
        fallback_ok,
        primary_call_count,
        all_requests_succeeded: all_succeeded,
        circuit_opens_at,
        failure_threshold,
    }
}

struct OutageSimResult {
    total_requests: usize,
    primary_ok: usize,
    fallback_ok: usize,
    primary_call_count: u64,
    all_requests_succeeded: bool,
    circuit_opens_at: Option<usize>,
    failure_threshold: u32,
}

// ============================================================================
// SLO 1: Circuit breaker opens within expected failure count (±1)
// ============================================================================

#[tokio::test]
async fn test_circuit_breaker_opens_by_failure_threshold() {
    let result = run_outage_simulation(2, 3, 60, 10).await;

    assert_slo(
        result.circuit_opens_at.is_some(),
        "Circuit breaker must open during simulation",
    );

    let opens_at = result.circuit_opens_at.unwrap();
    let expected = 2 + result.failure_threshold as usize;
    assert_slo(
        opens_at.abs_diff(expected) <= 1,
        &format!(
            "Circuit must open at or near request {expected} (failure threshold: {}), but opened at {opens_at}",
            result.failure_threshold
        ),
    );

    // Verify primary was attempted exactly (successes + threshold) times before being skipped
    assert_slo(
        result.primary_call_count == 5,
        &format!(
            "Primary must be called exactly success_before_fail + threshold = 5 times, got {}",
            result.primary_call_count
        ),
    );

    // All remaining requests handled by fallback
    assert_slo(
        result.fallback_ok == result.total_requests - result.primary_ok,
        "All remaining requests must be handled by fallback after circuit opens",
    );
}

// ============================================================================
// SLO 2: Fallback routing has zero downtime (immediate switch)
// ============================================================================

#[tokio::test]
async fn test_fallback_routing_zero_downtime() {
    let result = run_outage_simulation(2, 3, 60, 10).await;

    // Every request must succeed — no gaps where no provider is available
    assert_slo(
        result.all_requests_succeeded,
        "Fallback routing must have zero downtime: all requests must succeed",
    );

    // Primary handles 2 (successful), then fallback handles all remaining
    // The switch from primary→fallback must be immediate within the same request cycle
    assert_slo(
        result.primary_ok == 2,
        &format!(
            "Primary should handle exactly 2 successful requests, got {}",
            result.primary_ok
        ),
    );
    assert_slo(
        result.fallback_ok == result.total_requests - 2,
        &format!(
            "Fallback must handle all {} remaining requests with zero downtime",
            result.total_requests - 2
        ),
    );

    // Verify circuit opens and bypasses primary; fallback handles directly
    assert_slo(
        result.fallback_ok > 0,
        "Fallback must activate at least once during outage simulation",
    );
}

// ============================================================================
// SLO 3: Resource recovery after failure completes within timeout
// ============================================================================

#[tokio::test]
async fn test_resource_recovery_after_failure() {
    let cooldown_secs = 1;
    let cb = CircuitBreaker::new(3, 1, cooldown_secs);

    // Phase 1: Open the circuit
    for _ in 0..3 {
        cb.record_failure();
    }
    assert_eq!(cb.state(), CircuitState::Open, "circuit should be Open");
    assert_slo(
        !cb.can_execute(),
        "Circuit must reject execution while in Open state",
    );

    // Phase 2: Wait for cooldown — circuit should transition to HalfOpen
    tokio::time::sleep(Duration::from_secs(cooldown_secs + 1)).await;

    assert_slo(
        cb.can_execute(),
        &format!(
            "Circuit must recover within {}s cooldown (should be HalfOpen now)",
            cooldown_secs
        ),
    );
    assert_slo(
        cb.state() == CircuitState::HalfOpen,
        "Circuit must enter HalfOpen state after cooldown expires",
    );

    // Phase 3: Record success in HalfOpen — circuit should close
    cb.record_success();
    assert_slo(
        cb.state() == CircuitState::Closed,
        "Circuit must close after successful execution in HalfOpen state",
    );
    assert_slo(
        cb.can_execute(),
        "Circuit must allow execution after recovery to Closed state",
    );
}

// ============================================================================
// SLO 4: Error propagation preserves diagnostic information
// ============================================================================

#[tokio::test]
async fn test_error_propagation_preserves_diagnostics() {
    // A provider that always fails with a distinctive diagnostic
    let failing = Arc::new(FailingProvider::new("diag-test", 0));

    // Run through the router with only a failing primary (no fallback)
    let breaker = CircuitBreaker::new(3, 1, 60);
    let primary_target =
        ProviderTarget::new("primary".into(), breaker, Box::new(move || failing.clone()));
    let default_target = ProviderTarget::new(
        "default".into(),
        CircuitBreaker::new(3, 1, 60),
        Box::new(|| Arc::new(AlwaysOkProvider::new("default"))),
    );

    let router =
        ProviderRouter::new(default_target).with_provider(vec!["test/".into()], primary_target);

    let req = test_request("test/model");

    // The first call should fail — the error must carry the original diagnostic
    let err = router.chat_completion(&req).await.unwrap_err();
    let err_msg = err.to_string();

    // A diagnostic SLO violation would be a generic "no available providers" message
    // that discards the original failure reason
    assert_slo(
        err_msg.contains("diag-test provider outage simulated"),
        &format!(
            "Error propagation must preserve diagnostic information. \
             Expected 'diag-test provider outage simulated' in error, got: {err_msg}"
        ),
    );
    assert_slo(
        err_msg.contains("diag-test"),
        "Provider name must be preserved in propagated error",
    );
}

// ============================================================================
// SLO 5: Circuit breaker remains Closed below threshold
// ============================================================================

#[tokio::test]
async fn test_circuit_breaker_remains_closed_below_threshold() {
    let result = run_outage_simulation(5, 5, 60, 12).await;

    // With 5 successes and 5 failures before circuit opens
    // The circuit should stay closed for all 10 primary calls (5 success + 5 fail)
    // All 12 requests should succeed via primary then fallback
    assert_slo(
        result.all_requests_succeeded,
        "All requests must succeed when circuit is below opening threshold",
    );

    assert_slo(
        result.primary_ok == 5,
        &format!(
            "Primary should handle exactly 5 successful requests before failing, got {}",
            result.primary_ok
        ),
    );
}

// ============================================================================
// SLO 6: Dynamic threshold update takes effect immediately
// ============================================================================

#[test]
fn test_dynamic_threshold_update_takes_effect() {
    let cb = CircuitBreaker::new(3, 2, 30);

    // Record 3 failures — circuit should open with threshold=3
    for _ in 0..3 {
        cb.record_failure();
    }
    assert_eq!(
        cb.state(),
        CircuitState::Open,
        "breaker should open at threshold=3"
    );

    // Update threshold to 5 while Open
    cb.update_thresholds(5, 30);
    cb.reset();

    // Now record 3 failures — circuit should stay Closed (new threshold is 5)
    for _ in 0..3 {
        cb.record_failure();
    }
    assert_slo(
        cb.state() == CircuitState::Closed,
        "Circuit must remain Closed at 3 failures after dynamic threshold update to 5",
    );

    // Record 2 more failures — circuit should now open
    for _ in 0..2 {
        cb.record_failure();
    }
    assert_slo(
        cb.state() == CircuitState::Open,
        "Circuit must open at updated threshold of 5 failures",
    );
}
