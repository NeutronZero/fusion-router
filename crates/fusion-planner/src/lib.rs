use fusion_core::PlatformError;
use fusion_ir::WorkflowIR;
use fusion_kernel::{CapabilityRegistry, CapabilitySystem};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionIntent {
    Quality,
    Speed,
    Balanced,
    Cheap,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlannerKind {
    Static,
    Dynamic,
    CapabilityBased,
}

pub struct PlannerContract {
    pub intent: String,
    pub execution_intent: ExecutionIntent,
}

pub struct PlannerService {
    capability_system: CapabilitySystem,
    capability_registry: CapabilityRegistry,
}

impl PlannerService {
    pub fn new(capability_system: CapabilitySystem) -> Self {
        Self {
            capability_system,
            capability_registry: CapabilityRegistry::new(),
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
        let _ = &self.capability_registry;
        let _ = &execution_intent;

        fusion_ir::WorkflowBuilder::new()
            .task("n1", "CodeGeneration")
            .map_err(|e| PlatformError::Planner {
                code: "BUILDER_ERR".to_string(),
                message: e.to_string(),
                recovery_suggestion: "Check node creation".to_string(),
            })?
            .output("n2")
            .map_err(|e| PlatformError::Planner {
                code: "BUILDER_ERR".to_string(),
                message: e.to_string(),
                recovery_suggestion: "Check output node creation".to_string(),
            })?
            .sequential("n1", "n2")
            .map_err(|e| PlatformError::Planner {
                code: "BUILDER_ERR".to_string(),
                message: e.to_string(),
                recovery_suggestion: "Check edge creation".to_string(),
            })?
            .build()
            .map_err(|e| PlatformError::Planner {
                code: "BUILDER_ERR".to_string(),
                message: e.to_string(),
                recovery_suggestion: "Check workflow validation".to_string(),
            })
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
