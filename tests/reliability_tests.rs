use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use fusion_plugin_api::{
    CapabilityContract, CapabilityExecutor, CapabilityId, CapabilityInstance,
    ExecutionError, ExecutionResult,
};
use fusion_router::executor::capability_executor::CapabilityExecutorEngine;
use fusion_router::scheduler::connector_resolver::{Connector, ConnectorDescriptor, ConnectorResolver};
use fusion_router::types::execution_context::ExecutionContext;
use serde_json::json;

/// Shared atomic counter used to alternate between success and failure.
#[derive(Clone)]
struct CallCounter(Arc<AtomicUsize>);

impl CallCounter {
    fn new() -> Self {
        Self(Arc::new(AtomicUsize::new(0)))
    }

    fn next(&self) -> usize {
        self.0.fetch_add(1, Ordering::SeqCst)
    }
}

/// A connector that alternates between success and failure on each execution call.
struct FlakyConnector {
    counter: CallCounter,
}

/// The executor returned by `FlakyConnector`, sharing its call counter.
struct FlakyExecutor {
    counter: CallCounter,
}

#[async_trait]
impl CapabilityExecutor for FlakyExecutor {
    async fn execute(
        &self,
        instance: &CapabilityInstance,
        _input: serde_json::Value,
    ) -> Result<ExecutionResult, ExecutionError> {
        let count = self.counter.next();
        if count % 2 == 0 {
            Ok(ExecutionResult {
                outputs: json!({"result": "ok"}),
                metrics: std::collections::HashMap::new(),
            })
        } else {
            Err(ExecutionError {
                connector: "flaky".into(),
                capability: instance.contract.id.clone(),
                reason: "simulated failure".into(),
                retryable: true,
            })
        }
    }
}

impl Connector for FlakyConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            name: "flaky".into(),
            version: semver::Version::new(0, 10, 0),
            supported_capabilities: vec![CapabilityId::new("flaky.test")],
        }
    }

    fn executor(&self) -> Arc<dyn CapabilityExecutor> {
        Arc::new(FlakyExecutor {
            counter: self.counter.clone(),
        })
    }
}

fn make_instance() -> CapabilityInstance {
    CapabilityInstance {
        contract: CapabilityContract {
            id: CapabilityId::new("flaky.test"),
            version: semver::Version::parse("0.1.0").unwrap(),
            description: "flaky test capability".into(),
            inputs_schema: json!({}),
            outputs_schema: json!({}),
            permissions: vec![],
            estimated_cost_usd: 0.0,
            estimated_latency_ms: 1,
            reliability_score: 0.5,
            supports_streaming: false,
        },
        runtime_params: json!({}),
    }
}

#[tokio::test]
async fn test_flaky_connector_alternates_success_failure() {
    let resolver = ConnectorResolver::new();
    let connector = FlakyConnector {
        counter: CallCounter::new(),
    };
    resolver
        .register_connector(Arc::new(connector))
        .expect("registration should succeed");

    let engine = CapabilityExecutorEngine::new(resolver);
    let instance = make_instance();

    // First call — succeeds (count=0, even)
    let ctx = ExecutionContext::new(instance.clone(), "flaky".into(), json!({}));
    let result = engine.execute_capability(&ctx).await;
    assert!(result.is_ok(), "call 0 (even) should succeed");

    // Second call — fails (count=1, odd)
    let ctx = ExecutionContext::new(instance.clone(), "flaky".into(), json!({}));
    let err = engine
        .execute_capability(&ctx)
        .await
        .expect_err("call 1 (odd) should fail");
    assert_eq!(err.connector, "flaky");
    assert_eq!(err.capability.as_str(), "flaky.test");
    assert!(err.reason.contains("simulated failure"));
    assert!(err.retryable, "failure should be marked retryable");

    // Third call — succeeds again (count=2, even)
    let ctx = ExecutionContext::new(instance.clone(), "flaky".into(), json!({}));
    let result = engine.execute_capability(&ctx).await;
    assert!(result.is_ok(), "call 2 (even) should succeed");

    // Fourth call — fails again (count=3, odd)
    let ctx = ExecutionContext::new(instance, "flaky".into(), json!({}));
    let err = engine
        .execute_capability(&ctx)
        .await
        .expect_err("call 3 (odd) should fail");
    assert_eq!(err.connector, "flaky");
    assert!(err.retryable);
}

#[tokio::test]
async fn test_no_panic_or_resource_leak_on_connector_failure() {
    let resolver = ConnectorResolver::new();
    let connector = FlakyConnector {
        counter: CallCounter::new(),
    };
    resolver
        .register_connector(Arc::new(connector))
        .expect("registration should succeed");

    let engine = CapabilityExecutorEngine::new(resolver.clone());
    let instance = make_instance();

    // Run 20 calls — alternating success/failure, verify no panic
    for i in 0..20 {
        let ctx = ExecutionContext::new(instance.clone(), "flaky".into(), json!({}));
        let result = engine.execute_capability(&ctx).await;
        if i % 2 == 0 {
            assert!(result.is_ok(), "call {} (even) should succeed", i);
        } else {
            assert!(result.is_err(), "call {} (odd) should fail", i);
        }
    }

    // Verify resolver state is intact — no resource leak
    let names = resolver.connector_names();
    assert_eq!(names.len(), 1, "connector should still be registered");
    assert_eq!(names[0], "flaky");

    let bound = resolver.bind(&instance);
    assert!(
        bound.is_ok(),
        "resolver should still bind after repeated failures"
    );

    // Verify executor from fresh bind still works
    let engine2 = CapabilityExecutorEngine::new(resolver);
    let ctx = ExecutionContext::new(instance, "flaky".into(), json!({}));
    let result = engine2.execute_capability(&ctx).await;
    assert!(result.is_ok(), "call after fresh bind should succeed (even count)");
}
