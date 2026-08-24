use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{PrimitiveGraph, StrategyIR};
use crate::types::{ArtifactKind, RetryPolicy};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Parallelism {
    Sequential,
    Fixed(u32),
    Unlimited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamingMode {
    None,
    IncrementalArtifacts,
    IncrementalReduction,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyDescriptor {
    pub name: String,
    pub parallelism: Parallelism,
    pub requires_barrier: bool,
    pub supports_streaming: StreamingMode,
    pub retry_policy: RetryPolicy,
    pub expected_outputs: Vec<ArtifactKind>,
}

pub trait Strategy: Send + Sync {
    fn descriptor(&self) -> StrategyDescriptor;
    fn lower(
        &self,
        ir: &StrategyIR,
        ctx: &CompilationContext,
    ) -> Result<PrimitiveGraph, CompilerDiagnostic>;
}

pub mod chain;
pub mod consensus;
pub mod debate;
pub mod fusion;
pub mod react;
pub mod reflection;
pub mod single;
