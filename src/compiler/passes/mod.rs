use async_trait::async_trait;
use crate::types::{CompilerError, WorkflowIR};

#[allow(unused_imports)]
pub mod policy;
#[allow(unused_imports)]
pub use policy::*;

#[async_trait]
pub trait CompilerPass: Send + Sync {
    fn name(&self) -> &str;
    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError>;
}
