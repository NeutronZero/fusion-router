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

use std::sync::Arc;
use crate::resource::ResourceManager;

pub struct BudgetOptimisationPass {
    pub resource_manager: Arc<dyn ResourceManager>,
}

#[async_trait]
impl CompilerPass for BudgetOptimisationPass {
    fn name(&self) -> &str {
        "budget_optimisation"
    }

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        let temp_graph = super::lower_to_graph(ir.clone())?;
        if !self.resource_manager.can_afford(&temp_graph).await {
            return Err(CompilerError::ValidationError("Budget exceeded".to_string()));
        }
        Ok(ir)
    }
}
