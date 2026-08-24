use crate::runtime::RuntimeError;
use async_trait::async_trait;

#[async_trait]
pub trait SandboxInstance: Send {
    async fn invoke(&mut self, input: &[u8]) -> Result<Vec<u8>, RuntimeError>;
    fn reset(&mut self) -> Result<(), RuntimeError>;
    fn memory_usage(&self) -> u64;
    fn fuel_consumed(&self) -> u64;
}
