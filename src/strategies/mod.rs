use crate::types::{ExecutionNode, ExecutionSubgraph, RetryPolicy, ArtifactKind};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{StrategyIR, PrimitiveGraph};
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

#[derive(Debug, Clone)]
pub struct StrategyDescriptor {
    pub name: &'static str,
    pub parallelism: Parallelism,
    pub requires_barrier: bool,
    pub supports_streaming: StreamingMode,
    pub retry_policy: RetryPolicy,
    pub expected_outputs: Vec<ArtifactKind>,
}

pub trait Strategy: Send + Sync {
    fn descriptor(&self) -> StrategyDescriptor;
    fn lower(&self, ir: &StrategyIR, ctx: &CompilationContext) -> Result<PrimitiveGraph, CompilerDiagnostic>;
    fn apply(&self, node: &ExecutionNode) -> ExecutionSubgraph;
}

pub mod single;
pub mod consensus;
pub mod reflection;
pub mod chain;
pub mod react;
pub mod debate;
pub mod fusion;
