use async_trait::async_trait;
use crate::types::{CompilerError, WorkflowIR};

pub mod legacy_passes;
pub use legacy_passes::*;

#[derive(Default)]
pub struct PassManager {
    pub passes: Vec<Box<dyn CompilerPass + Send + Sync>>,
}

impl PassManager {
    pub fn new() -> Self {
        Self { passes: Vec::new() }
    }

    pub fn add_pass(&mut self, pass: Box<dyn CompilerPass + Send + Sync>) {
        self.passes.push(pass);
    }
}

#[async_trait]
pub trait CompilerPass: Send + Sync {
    fn name(&self) -> &str;
    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError>;
}
