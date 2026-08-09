//! Budget optimisation pass — checks whether an IR fits within resource quota.
//!
//! Ported from the monolith's `src/compiler/passes/legacy_passes.rs` (lines 76-107).
//! The pass is thin: it builds a throwaway budget check from the IR's estimated
//! cost/tokens and calls `ResourceManager::can_afford()`.
//!
//! **Plumbing test, not budget-logic test:** Tests verify the pass calls
//! `can_afford()` with the right arguments and propagates the result correctly.
//! Real production accounting stays in the monolith's `DefaultResourceManager`.
//! The `StubResourceManager` from `fusion_kernel` tracks state for accumulation
//! tests, but its logic is simplified — not a production substitute.

use std::sync::Arc;
use fusion_kernel::resource::ResourceManager;
use fusion_ir::WorkflowIR;
use crate::{CompilerPass, PlatformError};

/// Budget optimisation pass — rejects IRs that exceed resource quota.
pub struct BudgetOptimisationPass {
    pub resource_manager: Arc<dyn ResourceManager>,
}

impl BudgetOptimisationPass {
    pub fn new(resource_manager: Arc<dyn ResourceManager>) -> Self {
        Self { resource_manager }
    }

    /// Checks whether the IR fits within budget. Returns Ok(()) if affordable,
    /// Err if not. Pass-through — does not modify the IR.
    pub async fn check(&self, ir: &WorkflowIR) -> Result<(), BudgetError> {
        let estimated_cost = ir.metadata().estimated_cost;
        let estimated_tokens = ir.metadata().estimated_tokens;

        if !self.resource_manager.can_afford(estimated_cost, estimated_tokens).await {
            return Err(BudgetError::Exceeded {
                estimated_cost,
                estimated_tokens,
            });
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl CompilerPass for BudgetOptimisationPass {
    fn name(&self) -> &str {
        "Budget Optimisation"
    }

    async fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        self.check(ir).await.map_err(|e| PlatformError::Compiler {
            code: "BUDGET_EXCEEDED".to_string(),
            message: e.to_string(),
            recovery_suggestion: "Reduce workflow cost or increase resource quota".to_string(),
        })?;
        Ok(ir.clone())
    }
}

/// Errors from budget optimisation.
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetError {
    Exceeded {
        estimated_cost: f64,
        estimated_tokens: u64,
    },
}

impl std::fmt::Display for BudgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BudgetError::Exceeded { estimated_cost, estimated_tokens } => {
                write!(
                    f,
                    "Budget exceeded: estimated cost ${:.4}, estimated tokens {}",
                    estimated_cost, estimated_tokens
                )
            }
        }
    }
}

impl std::error::Error for BudgetError {}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_kernel::resource::StubResourceManager;

    fn make_ir_with_budget(cost: f64, tokens: u64) -> WorkflowIR {
        // Set metadata by re-serializing with desired values
        // WorkflowIR doesn't expose mutable metadata, so we build via JSON
        let json = serde_json::json!({
            "version": 1,
            "workflow_id": "00000000-0000-0000-0000-000000000000",
            "nodes": [{"id": "n1", "kind": "Task", "capability": "CodeGeneration", "config": {}}],
            "edges": [],
            "metadata": {
                "policy_applied": [],
                "estimated_cost": cost,
                "estimated_tokens": tokens
            }
        });
        fusion_ir::WorkflowIR::from_json(&json.to_string()).unwrap()
    }

    // -----------------------------------------------------------------------
    // Plumbing tests — pass calls can_afford, propagates result
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn budget_pass_plumbing_allows_under_quota() {
        let stub = StubResourceManager::new(100.0, 1_000_000);
        let pass = BudgetOptimisationPass::new(Arc::new(stub) as Arc<dyn ResourceManager>);

        let ir = make_ir_with_budget(0.01, 1000);
        assert!(pass.check(&ir).await.is_ok());
    }

    #[tokio::test]
    async fn budget_pass_plumbing_rejects_over_quota() {
        let stub = StubResourceManager::new(0.001, 100); // Tiny quota
        let pass = BudgetOptimisationPass::new(Arc::new(stub) as Arc<dyn ResourceManager>);

        let ir = make_ir_with_budget(10.0, 10_000); // Way over quota
        let err = pass.check(&ir).await.unwrap_err();
        assert!(matches!(err, BudgetError::Exceeded { .. }));
    }

    // -----------------------------------------------------------------------
    // Accumulation tests — second call sees state from first
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn budget_pass_accumulates_spend() {
        let stub: Arc<StubResourceManager> = Arc::new(StubResourceManager::new(0.10, 10_000)); // $0.10 quota, 10k tokens
        let pass = BudgetOptimisationPass::new(Arc::clone(&stub) as Arc<dyn ResourceManager>);

        // First IR: $0.05, 5000 tokens — fits ($0.05 < $0.10)
        let ir1 = make_ir_with_budget(0.05, 5000);
        assert!(pass.check(&ir1).await.is_ok());

        // Simulate the spend happening (pass doesn't do this — executor does)
        stub.simulate_spend(50_000, 5000); // $50 millicosts = $0.05 spent

        // Second IR: $0.06, 5000 tokens — now over because $0.05 + $0.06 = $0.11 > $0.10
        let ir2 = make_ir_with_budget(0.06, 5000);
        let err = pass.check(&ir2).await.unwrap_err();
        assert!(matches!(err, BudgetError::Exceeded { .. }));
    }

    #[tokio::test]
    async fn budget_pass_shared_state_between_instances() {
        let stub: Arc<StubResourceManager> = Arc::new(StubResourceManager::new(0.10, 10_000));

        // Two passes sharing the same stub (coerce to trait object)
        let pass1 = BudgetOptimisationPass::new(Arc::clone(&stub) as Arc<dyn ResourceManager>);
        let pass2 = BudgetOptimisationPass::new(Arc::clone(&stub) as Arc<dyn ResourceManager>);

        // pass1: $0.05, 5000 tokens — fits
        let ir1 = make_ir_with_budget(0.05, 5000);
        assert!(pass1.check(&ir1).await.is_ok());

        // Simulate spend
        stub.simulate_spend(50_000, 5000);

        // pass2: $0.06, 5000 tokens — over quota ($0.11 > $0.10)
        let ir2 = make_ir_with_budget(0.06, 5000);
        let err = pass2.check(&ir2).await.unwrap_err();
        assert!(matches!(err, BudgetError::Exceeded { .. }));
    }
}
