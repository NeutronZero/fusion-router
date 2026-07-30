use async_trait::async_trait;
use crate::runtime::context::RuntimeContext;
use crate::runtime::sandbox_instance::SandboxInstance;
use crate::runtime::RuntimeError;

#[async_trait]
pub trait SandboxRuntime: Send + Sync {
    fn name(&self) -> &'static str;
    async fn instantiate(
        &self,
        module_bytes: &[u8],
        ctx: RuntimeContext,
    ) -> Result<Box<dyn SandboxInstance>, RuntimeError>;
}
