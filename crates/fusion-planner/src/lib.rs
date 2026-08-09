pub mod capability;

use fusion_core::{ModelCatalog, PlatformError};
use fusion_ir::{WorkflowBuilder, WorkflowIR};
use fusion_kernel::{CapabilityCatalog, CapabilitySystem};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExecutionIntent {
    Quality,
    Speed,
    Balanced,
    Exhaustive,
    Constrained { max_cost_usd: Option<f64> },
}

pub struct IntentPlanner {
    pub model_catalog: ModelCatalog,
}

impl IntentPlanner {
    pub fn new(model_catalog: ModelCatalog) -> Self {
        Self { model_catalog }
    }

    pub fn build_quality(&self) -> Result<WorkflowIR, PlatformError> {
        WorkflowBuilder::new()
            .task("n1", "CodeGeneration")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .task("n2", "CodeGeneration")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .task("n3", "CodeGeneration")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .review("n4", "Reviewer")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .task("n5", "Reflection")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .sequential("n1", "n2").map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Edge".into() })?
            .sequential("n2", "n3").map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Edge".into() })?
            .sequential("n3", "n4").map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Edge".into() })?
            .sequential("n4", "n5").map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Edge".into() })?
            .build()
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Build".into() })
    }

    pub fn build_speed(&self) -> Result<WorkflowIR, PlatformError> {
        WorkflowBuilder::new()
            .task("n1", "CodeGeneration")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .output("n2")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .sequential("n1", "n2")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Edge".into() })?
            .build()
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Build".into() })
    }

    pub fn build_balanced(&self) -> Result<WorkflowIR, PlatformError> {
        WorkflowBuilder::new()
            .task("n1", "CodeGeneration")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .task("n2", "CodeGeneration")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .review("n3", "Reviewer")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .sequential("n1", "n2").map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Edge".into() })?
            .sequential("n2", "n3").map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Edge".into() })?
            .build()
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Build".into() })
    }

    pub fn build_exhaustive(&self) -> Result<WorkflowIR, PlatformError> {
        WorkflowBuilder::new()
            .task("n1", "CodeGeneration")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .task("n2", "CodeGeneration")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .task("n3", "CodeGeneration")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .review("n4", "Reviewer")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .task("n5", "Reflection")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .review("n6", "Reviewer")
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Check node".into() })?
            .sequential("n1", "n2").map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Edge".into() })?
            .sequential("n2", "n3").map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Edge".into() })?
            .sequential("n3", "n4").map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Edge".into() })?
            .sequential("n4", "n5").map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Edge".into() })?
            .sequential("n5", "n6").map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Edge".into() })?
            .build()
            .map_err(|e| PlatformError::Planner { code: "BUILDER_ERR".to_string(), message: e.to_string(), recovery_suggestion: "Build".into() })
    }

    pub fn plan_intent(&self, execution_intent: &ExecutionIntent) -> Result<WorkflowIR, PlatformError> {
        match execution_intent {
            ExecutionIntent::Quality => self.build_quality(),
            ExecutionIntent::Speed => self.build_speed(),
            ExecutionIntent::Balanced => self.build_balanced(),
            ExecutionIntent::Exhaustive => self.build_exhaustive(),
            ExecutionIntent::Constrained { max_cost_usd } => {
                if let Some(cost) = max_cost_usd {
                    if *cost < 0.02 {
                        return self.build_speed();
                    }
                }
                self.build_balanced()
            }
        }
    }
}

pub struct PlannerService {
    capability_system: CapabilitySystem,
    capability_catalog: CapabilityCatalog,
    intent_planner: IntentPlanner,
}

impl PlannerService {
    pub fn new(capability_system: CapabilitySystem) -> Self {
        Self {
            capability_system,
            capability_catalog: CapabilityCatalog::new(),
            intent_planner: IntentPlanner::new(ModelCatalog::default()),
        }
    }

    pub fn plan(&self, intent: &str) -> Result<WorkflowIR, PlatformError> {
        self.plan_with_intent(intent, ExecutionIntent::Balanced)
    }

    pub fn plan_with_intent(&self, intent: &str, execution_intent: ExecutionIntent) -> Result<WorkflowIR, PlatformError> {
        if intent.is_empty() {
            return Err(PlatformError::Planner {
                code: "EMPTY_INTENT".to_string(),
                message: "Intent cannot be empty".to_string(),
                recovery_suggestion: "Provide a valid natural language prompt or workflow spec".to_string(),
            });
        }
        let _ = &self.capability_system;
        let _ = &self.capability_catalog;
        self.intent_planner.plan_intent(&execution_intent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_planner_service_with_intents() {
        let system = CapabilitySystem::new();
        let planner = PlannerService::new(system);
        let ir = planner.plan_with_intent("Build web application", ExecutionIntent::Quality).expect("Plan");
        assert_eq!(ir.nodes().len(), 2);
    }
}
