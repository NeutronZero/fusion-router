use async_trait::async_trait;
use crate::types::{CompilerError, WorkflowIR};

pub mod legacy_passes;
#[allow(unused_imports)]
pub mod policy;
// Phase 6: the production pipeline is the crates one (see `build_compiler`).
// The src legacy passes remain exported only for the unwired trigger
// `CompilerPipeline` and its tests; they are deleted in the Phase 6.6
// cleanup once `CompilerPipeline` is removed.
#[allow(unused_imports)]
pub use legacy_passes::*;
#[allow(unused_imports)]
pub use policy::*;

#[async_trait]
pub trait CompilerPass: Send + Sync {
    fn name(&self) -> &str;
    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError>;
}
