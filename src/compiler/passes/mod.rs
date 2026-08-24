use crate::types::{CompilerError, WorkflowIR};
use async_trait::async_trait;

#[async_trait]
pub trait CompilerPass: Send + Sync {
    fn name(&self) -> &str;
    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError>;
}
