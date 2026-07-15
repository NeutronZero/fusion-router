use async_trait::async_trait;
use crate::types::CompilerError;
use super::CompilerPass;
use crate::types::WorkflowIR;

pub struct ConstraintValidationPass;

#[async_trait]
impl CompilerPass for ConstraintValidationPass {
    fn name(&self) -> &str {
        "constraint_validation"
    }

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        if ir.nodes.is_empty() {
            return Err(CompilerError::ValidationError("IR must have at least one node".to_string()));
        }
        Ok(ir)
    }
}

pub struct ModelResolutionPass;

#[async_trait]
impl CompilerPass for ModelResolutionPass {
    fn name(&self) -> &str {
        "model_resolution"
    }

    async fn apply(&self, mut ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        for node in &mut ir.nodes {
            if node.model.is_none() {
                node.model = Some("claude-sonnet-4-20250514".to_string());
            }
        }
        Ok(ir)
    }
}

pub struct BudgetOptimisationPass;

#[async_trait]
impl CompilerPass for BudgetOptimisationPass {
    fn name(&self) -> &str {
        "budget_optimisation"
    }

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        Ok(ir)
    }
}
