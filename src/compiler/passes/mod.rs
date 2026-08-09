use async_trait::async_trait;
use crate::types::{CompilerError, WorkflowIR};

pub mod legacy_passes;
#[allow(unused_imports)]
pub mod policy;
pub use legacy_passes::*;
#[allow(unused_imports)]
pub use policy::*;

#[async_trait]
pub trait CompilerPass: Send + Sync {
    fn name(&self) -> &str;
    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError>;
}
