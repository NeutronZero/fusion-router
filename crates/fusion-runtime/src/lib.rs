use fusion_core::{ExecutionId, ExecutionState, PlatformError};

pub struct RuntimeEngine;

impl RuntimeEngine {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self, id: ExecutionId) -> Result<ExecutionState, PlatformError> {
        let _ = id;
        Ok(ExecutionState::Completed)
    }
}

impl Default for RuntimeEngine {
    fn default() -> Self {
        Self::new()
    }
}
