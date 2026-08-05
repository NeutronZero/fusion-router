use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::abi::ExecutionAbi;
use crate::target::ExecutionTarget;

pub mod local_runtime;

/// Runtime execution contract (v0.13 contract 5).
/// The runtime executes ABIs; it never interprets user intent.
#[async_trait]
pub trait ExecutionRuntimeInterface: Send + Sync {
    fn name(&self) -> &'static str;

    async fn execute(
        &self,
        abi: &ExecutionAbi,
        target: &ExecutionTarget,
    ) -> Result<ExecutionAbiResult, EriError>;

    async fn cancel(&self, execution_id: &Uuid) -> Result<(), EriError>;

    async fn state(&self, execution_id: &Uuid) -> Result<ExecutionState, EriError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionState {
    Planned,
    Compiled,
    Queued,
    Running,
    Waiting,
    Retrying,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionAbiResult {
    pub execution_id: Uuid,
    pub state: ExecutionState,
    pub outputs: HashMap<String, serde_json::Value>,
    pub metrics: HashMap<String, f64>,
}

#[derive(Debug, thiserror::Error)]
pub enum EriError {
    #[error("ABI version not supported: {0}")]
    UnsupportedAbiVersion(u16),
    #[error("Execution not found: {0}")]
    NotFound(Uuid),
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_states_round_trip() {
        for state in [
            ExecutionState::Planned,
            ExecutionState::Compiled,
            ExecutionState::Queued,
            ExecutionState::Running,
            ExecutionState::Waiting,
            ExecutionState::Retrying,
            ExecutionState::Succeeded,
            ExecutionState::Failed,
            ExecutionState::Cancelled,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let back: ExecutionState = serde_json::from_str(&json).unwrap();
            assert_eq!(back, state);
        }
    }

    #[test]
    fn abi_result_serde_round_trip() {
        let mut outputs = HashMap::new();
        outputs.insert("out".into(), serde_json::json!({"ok": true}));
        let result = ExecutionAbiResult {
            execution_id: Uuid::new_v4(),
            state: ExecutionState::Succeeded,
            outputs,
            metrics: HashMap::from([("latency_ms".to_string(), 42.0)]),
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: ExecutionAbiResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.state, ExecutionState::Succeeded);
        assert_eq!(back.outputs["out"]["ok"], serde_json::json!(true));
    }

    #[test]
    fn eri_error_display() {
        assert_eq!(
            EriError::UnsupportedAbiVersion(2).to_string(),
            "ABI version not supported: 2"
        );
        assert!(EriError::NotFound(Uuid::nil())
            .to_string()
            .contains("Execution not found"));
    }

    /// Compile-time check: the trait is object-safe for Box<dyn ...>.
    #[test]
    fn eri_trait_object_safe() {
        fn _take(_rt: Box<dyn ExecutionRuntimeInterface>) {}
    }

    /// Compile-time check: the trait is Send + Sync.
    #[test]
    fn eri_is_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<Box<dyn ExecutionRuntimeInterface>>();
    }
}
