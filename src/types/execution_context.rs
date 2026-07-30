//! Phase 3B — `ExecutionContext`, `ExecutionState`, `ExecutionEvent`, & `ExecutionTrace` (`src/types/execution_context.rs`)
//!
//! Standardized runtime container, lifecycle state machine, append-only event stream, and provenance trace.

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use fusion_plugin_api::{CapabilityId, CapabilityInstance};

/// Runtime lifecycle state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExecutionState {
    Pending,
    Resolved,
    Scheduled,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

/// Individual immutable event recorded during capability execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionEvent {
    ConnectorBound { connector: String, capability: CapabilityId },
    ExecutionStarted { timestamp_ms: u64 },
    PluginInvoked { plugin: String },
    PluginCompleted { status: String },
    RetryScheduled { attempt: u32 },
    ExecutionFinished { final_state: ExecutionState, timestamp_ms: u64 },
}

/// Append-only provenance execution trace recording runtime events.
#[derive(Debug, Clone)]
pub struct ExecutionTrace {
    pub trace_id: Uuid,
    events: Arc<Mutex<Vec<ExecutionEvent>>>,
}

impl Serialize for ExecutionTrace {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let events = self.events();
        let mut state = serializer.serialize_struct("ExecutionTrace", 2)?;
        state.serialize_field("trace_id", &self.trace_id)?;
        state.serialize_field("events", &events)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ExecutionTrace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            trace_id: Uuid,
            events: Vec<ExecutionEvent>,
        }
        let helper = Helper::deserialize(deserializer)?;
        Ok(Self {
            trace_id: helper.trace_id,
            events: Arc::new(Mutex::new(helper.events)),
        })
    }
}

impl ExecutionTrace {
    pub fn new(trace_id: Uuid) -> Self {
        Self {
            trace_id,
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Appends an event to the immutable event log.
    pub fn record(&self, event: ExecutionEvent) {
        let mut guard = self.events.lock();
        guard.push(event);
    }

    /// Returns a snapshot of recorded execution events.
    pub fn events(&self) -> Vec<ExecutionEvent> {
        let guard = self.events.lock();
        guard.clone()
    }
}

/// Immutable runtime container passed to the capability execution engine.
#[derive(Clone)]
pub struct ExecutionContext {
    pub execution_id: Uuid,
    pub capability_instance: CapabilityInstance,
    pub connector_name: String,
    pub inputs: serde_json::Value,
    pub metadata: HashMap<String, String>,
    pub deadline_ms: Option<u64>,
    pub state: Arc<Mutex<ExecutionState>>,
    pub trace: ExecutionTrace,
    pub config_generation: u64,
}

impl ExecutionContext {
    pub fn new(
        capability_instance: CapabilityInstance,
        connector_name: String,
        inputs: serde_json::Value,
    ) -> Self {
        let execution_id = Uuid::new_v4();
        let trace = ExecutionTrace::new(execution_id);

        trace.record(ExecutionEvent::ConnectorBound {
            connector: connector_name.clone(),
            capability: capability_instance.contract.id.clone(),
        });

        Self {
            execution_id,
            capability_instance,
            connector_name,
            inputs,
            metadata: HashMap::new(),
            deadline_ms: None,
            state: Arc::new(Mutex::new(ExecutionState::Pending)),
            trace,
            config_generation: 0,
        }
    }

    /// Updates execution state.
    pub fn set_state(&self, new_state: ExecutionState) {
        let mut guard = self.state.lock();
        *guard = new_state;
    }

    /// Returns current execution state.
    pub fn state(&self) -> ExecutionState {
        let guard = self.state.lock();
        *guard
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_execution_context_lifecycle() {
        let instance = fusion_plugin_api::CapabilityInstance {
            contract: fusion_plugin_api::CapabilityContract {
                id: fusion_plugin_api::CapabilityId::new("echo.text"),
                version: semver::Version::parse("0.1.0").unwrap(),
                description: "Test".into(),
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

        let ctx = ExecutionContext::new(instance, "echo".into(), json!({"text": "hi"}));
        assert_eq!(ctx.state(), ExecutionState::Pending);

        ctx.set_state(ExecutionState::Running);
        assert_eq!(ctx.state(), ExecutionState::Running);

        ctx.trace.record(ExecutionEvent::ExecutionStarted { timestamp_ms: 100 });
        assert_eq!(ctx.trace.events().len(), 2); // ConnectorBound + ExecutionStarted
    }
}
