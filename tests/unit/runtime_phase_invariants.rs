//! Executable Runtime Phase Invariants Test Suite & Determinism Validation
//!
//! Verifies runtime phase boundaries, execution state machine transitions, trace append-only invariant, and 100x Echo execution determinism.

use std::sync::Arc;
use fusion_plugin_api::{CapabilityContract, CapabilityId, CapabilityInstance};
use fusion_plugin_echo::EchoPlugin;
use fusion_router::executor::capability_executor::CapabilityExecutorEngine;
use fusion_router::scheduler::connector_resolver::{Connector, ConnectorDescriptor, ConnectorResolver};
use fusion_router::types::execution_context::{ExecutionContext, ExecutionEvent, ExecutionState};
use serde_json::json;

struct EchoConnector {
    plugin: Arc<EchoPlugin>,
}

impl Connector for EchoConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            name: "echo".into(),
            version: semver::Version::new(0, 10, 0),
            supported_capabilities: vec![
                CapabilityId::new("echo.text"),
                CapabilityId::new("echo.uppercase"),
            ],
        }
    }

    fn executor(&self) -> Arc<dyn fusion_plugin_api::CapabilityExecutor> {
        self.plugin.clone()
    }
}

fn create_engine() -> (CapabilityExecutorEngine, CapabilityContract) {
    let resolver = ConnectorResolver::new();
    let echo_conn = Arc::new(EchoConnector {
        plugin: Arc::new(EchoPlugin::new()),
    });
    resolver.register_connector(echo_conn).unwrap();

    let contract = CapabilityContract {
        id: CapabilityId::new("echo.uppercase"),
        version: semver::Version::parse("0.1.0").unwrap(),
        description: "Uppercase".into(),
        inputs_schema: json!({}),
        outputs_schema: json!({}),
            permissions: vec![],
            dependencies: vec![],
            estimated_cost_usd: 0.0,
        estimated_latency_ms: 1,
        reliability_score: 1.0,
        supports_streaming: false,
        traits: vec![],
    };

    (CapabilityExecutorEngine::new(resolver), contract)
}

#[tokio::test]
async fn runtime_invariant_execution_state_transitions() {
    let (engine, contract) = create_engine();
    let instance = CapabilityInstance {
        contract,
        runtime_params: json!({}),
    };

    let ctx = ExecutionContext::new(instance, "echo".into(), json!({"text": "test state"}));
    assert_eq!(ctx.state(), ExecutionState::Pending);

    let res = engine.execute_capability(&ctx).await.unwrap();
    assert_eq!(res.outputs["text"], "TEST STATE");
    assert_eq!(ctx.state(), ExecutionState::Succeeded);
}

#[tokio::test]
async fn runtime_invariant_trace_is_append_only() {
    let (engine, contract) = create_engine();
    let instance = CapabilityInstance {
        contract,
        runtime_params: json!({}),
    };

    let ctx = ExecutionContext::new(instance, "echo".into(), json!({"text": "trace invariant"}));
    let initial_count = ctx.trace.events().len(); // ConnectorBound

    let _ = engine.execute_capability(&ctx).await.unwrap();
    let final_events = ctx.trace.events();

    assert!(final_events.len() > initial_count);
    match &final_events[0] {
        ExecutionEvent::ConnectorBound { connector, .. } => assert_eq!(connector, "echo"),
        _ => panic!("Expected ConnectorBound first event"),
    }
}

#[tokio::test]
async fn runtime_invariant_100x_echo_execution_determinism() {
    let (engine, contract) = create_engine();
    let instance = CapabilityInstance {
        contract,
        runtime_params: json!({}),
    };

    for _ in 0..100 {
        let ctx = ExecutionContext::new(instance.clone(), "echo".into(), json!({"text": "fusion"}));
        let res = engine.execute_capability(&ctx).await.unwrap();

        // Output determinism
        assert_eq!(res.outputs["text"], "FUSION");
        // State determinism
        assert_eq!(ctx.state(), ExecutionState::Succeeded);
        // Event sequence count determinism
        assert_eq!(ctx.trace.events().len(), 5);
    }
}
