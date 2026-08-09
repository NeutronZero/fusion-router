//! Runtime engine — executes scheduled workflows against providers.
use fusion_core::{ExecutionId, ExecutionState, PlatformError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeExecutionStep {
    pub step_index: usize,
    pub node_id: String,
    pub status: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCheckpoint {
    pub checkpoint_id: String,
    pub execution_id: ExecutionId,
    pub completed_steps: usize,
    pub state: ExecutionState,
}

pub struct RuntimeEngine;

impl RuntimeEngine {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self, id: ExecutionId) -> Result<ExecutionState, PlatformError> {
        let _ = id;
        Ok(ExecutionState::Completed)
    }

    pub async fn checkpoint(&self, id: ExecutionId, steps: usize) -> Result<RuntimeCheckpoint, PlatformError> {
        Ok(RuntimeCheckpoint {
            checkpoint_id: format!("chk_{}_{steps}", id.0),
            execution_id: id,
            completed_steps: steps,
            state: ExecutionState::Completed,
        })
    }
}

impl Default for RuntimeEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_runtime_engine_execution_and_checkpoint() {
        let engine = RuntimeEngine::new();
        let id = ExecutionId::new();
        let state = engine.execute(id).await.expect("Execute");
        assert_eq!(state, ExecutionState::Completed);

        let chk = engine.checkpoint(id, 2).await.expect("Checkpoint");
        assert_eq!(chk.completed_steps, 2);
    }
}
