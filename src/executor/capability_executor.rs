//! Phase 3C — `CapabilityExecutorEngine` (`src/executor/capability_executor.rs`)
//!
//! Runtime execution engine executing capability invocations given an immutable `ExecutionContext`.

use fusion_plugin_api::{ExecutionError, ExecutionResult};
use crate::scheduler::connector_resolver::{BoundConnector, ConnectorResolver};
use crate::types::execution_context::{ExecutionContext, ExecutionEvent, ExecutionState};

pub struct CapabilityExecutorEngine {
    connector_resolver: ConnectorResolver,
}

impl CapabilityExecutorEngine {
    pub fn new(connector_resolver: ConnectorResolver) -> Self {
        Self { connector_resolver }
    }

    /// Late-binds and executes a capability instance given an execution payload.
    pub async fn execute_capability(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<ExecutionResult, ExecutionError> {
        ctx.set_state(ExecutionState::Running);
        ctx.trace.record(ExecutionEvent::ExecutionStarted { timestamp_ms: 0 });

        // 1. Late-bind connector via ConnectorResolver
        let bound: BoundConnector = self
            .connector_resolver
            .bind(&ctx.capability_instance)
            .map_err(|e| ExecutionError {
                connector: ctx.connector_name.clone(),
                capability: ctx.capability_instance.contract.id.clone(),
                reason: e,
                retryable: false,
            })?;

        ctx.trace.record(ExecutionEvent::PluginInvoked {
            plugin: bound.connector_descriptor.name.clone(),
        });

        // 2. Invoke physical executor
        match bound
            .executor
            .execute(&ctx.capability_instance, ctx.inputs.clone())
            .await
        {
            Ok(result) => {
                ctx.set_state(ExecutionState::Succeeded);
                ctx.trace.record(ExecutionEvent::PluginCompleted {
                    status: "Succeeded".into(),
                });
                ctx.trace.record(ExecutionEvent::ExecutionFinished {
                    final_state: ExecutionState::Succeeded,
                    timestamp_ms: 1,
                });
                Ok(result)
            }
            Err(err) => {
                ctx.set_state(ExecutionState::Failed);
                ctx.trace.record(ExecutionEvent::PluginCompleted {
                    status: "Failed".into(),
                });
                ctx.trace.record(ExecutionEvent::ExecutionFinished {
                    final_state: ExecutionState::Failed,
                    timestamp_ms: 1,
                });
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use fusion_plugin_api::{CapabilityContract, CapabilityId, CapabilityInstance};
    use fusion_plugin_echo::EchoPlugin;
    use crate::scheduler::connector_resolver::{Connector, ConnectorDescriptor};

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

    #[tokio::test]
    async fn test_capability_executor_engine_execution() {
        let resolver = ConnectorResolver::new();
        let echo_conn = Arc::new(EchoConnector {
            plugin: Arc::new(EchoPlugin::new()),
        });
        resolver.register_connector(echo_conn).unwrap();

        let engine = CapabilityExecutorEngine::new(resolver);

        let instance = CapabilityInstance {
            contract: CapabilityContract {
                id: CapabilityId::new("echo.uppercase"),
                version: semver::Version::parse("0.1.0").unwrap(),
                description: "Uppercase".into(),
                inputs_schema: serde_json::json!({}),
                outputs_schema: serde_json::json!({}),
                permissions: vec![],
                dependencies: vec![],
                estimated_cost_usd: 0.0,
                estimated_latency_ms: 1,
                reliability_score: 1.0,
                supports_streaming: false,
            },
            runtime_params: serde_json::json!({}),
        };

        let ctx = ExecutionContext::new(instance, "echo".into(), serde_json::json!({"text": "fusion router"}));
        let res = engine.execute_capability(&ctx).await.unwrap();

        assert_eq!(res.outputs["text"], "FUSION ROUTER");
        assert_eq!(ctx.state(), ExecutionState::Succeeded);
        assert_eq!(ctx.trace.events().len(), 5);
    }
}
