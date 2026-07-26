use crate::compiler::ir::PrimitiveGraph;
use crate::compiler::diagnostics::CompilerDiagnostic;

pub trait OptimizationPass: Send + Sync {
    fn name(&self) -> &str;
    fn optimize(&self, graph: PrimitiveGraph) -> Result<PrimitiveGraph, CompilerDiagnostic>;
}

#[derive(Default)]
pub struct OptimizationPipeline {
    passes: Vec<Box<dyn OptimizationPass>>,
}

impl OptimizationPipeline {
    pub fn new() -> Self {
        Self { passes: Vec::new() }
    }

    pub fn add_pass(&mut self, pass: Box<dyn OptimizationPass>) {
        self.passes.push(pass);
    }

    pub fn run(&self, mut graph: PrimitiveGraph) -> Result<PrimitiveGraph, CompilerDiagnostic> {
        for pass in &self.passes {
            graph = pass.optimize(graph)?;
        }
        Ok(graph)
    }
}
