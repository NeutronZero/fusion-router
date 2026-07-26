use fusion_router::providers::circuit_breaker::{CircuitBreaker, CircuitState};
use fusion_router::resource::budget::BudgetExceededError;
use fusion_router::resource::{BudgetEnvelope, DefaultResourceManager, ResourceGuard, ResourceManager};
use fusion_router::types::{ExecutionGraph, GraphMetadata, Quota};
use std::sync::Arc;
use uuid::Uuid;

#[test]
fn test_provider_circuit_breaker_fallback() {
    let cb = CircuitBreaker::new(3, 1, 60);
    assert_eq!(cb.state(), CircuitState::Closed);
    assert!(cb.can_execute());

    cb.record_failure();
    cb.record_failure();
    cb.record_failure();

    assert_eq!(cb.state(), CircuitState::Open);
    assert!(!cb.can_execute());

    let result = if cb.can_execute() {
        "primary"
    } else {
        "fallback"
    };
    assert_eq!(result, "fallback");

    cb.reset();
    assert_eq!(cb.state(), CircuitState::Closed);
    assert!(cb.can_execute());
}

#[tokio::test]
async fn test_resource_quota_exhausted_globally() {
    let quota = Quota {
        max_daily_cost: 1.0,
        max_daily_tokens: 100,
        max_concurrent: 5,
        provider_limits: std::collections::HashMap::new(),
    };
    let manager: Arc<dyn ResourceManager> = Arc::new(DefaultResourceManager::new(quota));

    let graph = ExecutionGraph {
        graph_id: Uuid::new_v4(),
        nodes: vec![],
        edges: vec![],
        metadata: GraphMetadata {
            estimated_cost: 0.5,
            estimated_tokens: 60,
            max_depth: 1,
            node_count: 0,
        },
        total_tokens: 60,
        total_cost: 500,
        primitive_graph_hash: 0,
    };

    assert!(manager.try_reserve(&graph).await);

    let graph2 = ExecutionGraph {
        graph_id: Uuid::new_v4(),
        metadata: GraphMetadata {
            estimated_cost: 0.8,
            estimated_tokens: 60,
            max_depth: 1,
            node_count: 0,
        },
        primitive_graph_hash: 0,
        ..graph
    };
    assert!(!manager.try_reserve(&graph2).await);
}

#[test]
fn test_budget_exceeded_per_request() {
    let env = BudgetEnvelope::new(1000, 100, 5);
    assert!(env.record_and_check(600, 30).is_ok());
    let err = env.record_and_check(500, 30).unwrap_err();
    assert_eq!(
        err,
        BudgetExceededError::Cost {
            spent: 1100,
            max: 1000
        }
    );
}

#[tokio::test]
async fn test_panic_in_executor_releases_quota() {
    let quota = Quota {
        max_daily_cost: 10.0,
        max_daily_tokens: 1000,
        max_concurrent: 10,
        provider_limits: std::collections::HashMap::new(),
    };
    let manager: Arc<dyn ResourceManager> = Arc::new(DefaultResourceManager::new(quota));

    let graph = ExecutionGraph {
        graph_id: Uuid::new_v4(),
        nodes: vec![],
        edges: vec![],
        metadata: GraphMetadata {
            estimated_cost: 2.0,
            estimated_tokens: 200,
            max_depth: 1,
            node_count: 0,
        },
        total_tokens: 200,
        total_cost: 2000,
        primitive_graph_hash: 0,
    };

    assert!(manager.try_reserve(&graph).await);
    assert_eq!(manager.spent_tokens(), 200);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = ResourceGuard::new(Uuid::new_v4(), graph.clone(), manager.clone());
        panic!("simulated executor panic");
    }));
    assert!(result.is_err());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(manager.spent_tokens(), 0);
}

#[tokio::test]
async fn test_client_cancellation_during_streaming() {
    let token = tokio_util::sync::CancellationToken::new();

    let handle = tokio::spawn({
        let token = token.clone();
        async move {
            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        return "cancelled";
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {}
                }
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!handle.is_finished());

    token.cancel();
    let result = handle.await.unwrap();
    assert_eq!(result, "cancelled");
}
