//! Phase 5A — `CompilerPipeline` & `PassManager` (`src/compiler/pipeline.rs`)
//!
//! Orchestrates compiler passes, validates prerequisites, records pass execution timing, and collects diagnostics.

use std::time::Instant;
use crate::compiler::context::CompilationContext;
use crate::compiler::passes::CompilerPass;
use crate::types::{CompilerError, WorkflowIR};

pub struct CompilerPipeline {
    passes: Vec<Box<dyn CompilerPass + Send + Sync>>,
}

impl CompilerPipeline {
    pub fn new() -> Self {
        Self { passes: Vec::new() }
    }

    pub fn add_pass(&mut self, pass: Box<dyn CompilerPass + Send + Sync>) {
        self.passes.push(pass);
    }

    /// Executes all registered compiler passes sequentially over an input `WorkflowIR`.
    pub async fn execute(
        &self,
        ir: WorkflowIR,
        _ctx: &CompilationContext,
    ) -> Result<WorkflowIR, CompilerError> {
        let mut current_ir = ir;

        for pass in &self.passes {
            let start = Instant::now();
            tracing::debug!(pass = %pass.name(), "executing compiler pass");

            current_ir = pass.apply(current_ir).await?;

            let elapsed = start.elapsed();
            tracing::debug!(pass = %pass.name(), latency_ms = elapsed.as_millis(), "completed compiler pass");
        }

        Ok(current_ir)
    }
}

impl Default for CompilerPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use crate::types::{IRNode, IRNodeKind, StrategyKind};

    #[tokio::test]
    async fn test_compiler_pipeline_execution() {
        use fusion_compiler::CompilerPass as _;

        struct PassAdapter(fusion_compiler::ConstraintValidationPass);

        #[async_trait::async_trait]
        impl CompilerPass for PassAdapter {
            fn name(&self) -> &str { self.0.name() }
            async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
                self.0.apply(ir).await
            }
        }

        let mut pipeline = CompilerPipeline::new();
        pipeline.add_pass(Box::new(PassAdapter(fusion_compiler::ConstraintValidationPass)));

        let input_ir = WorkflowIR {
            plan_id: Uuid::new_v4(),
            nodes: vec![IRNode {
                id: Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some("gpt-4o".into()),
                config: std::collections::HashMap::new(),
            }],
            edges: vec![],
            metadata: crate::types::IRMetadata {
                policy_applied: vec![],
                estimated_cost: 0.0,
                estimated_tokens: 10,
            },
        };

        let ctx = CompilationContext::new();

        let output_ir = pipeline.execute(input_ir, &ctx).await.unwrap();
        assert_eq!(output_ir.nodes.len(), 1);
    }
}
