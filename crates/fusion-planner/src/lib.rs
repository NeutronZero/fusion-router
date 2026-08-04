use fusion_core::PlatformError;
use fusion_ir::WorkflowIR;
use fusion_kernel::CapabilitySystem;

pub struct PlannerContract {
    pub intent: String,
}

pub struct PlannerService {
    capability_system: CapabilitySystem,
}

impl PlannerService {
    pub fn new(capability_system: CapabilitySystem) -> Self {
        Self { capability_system }
    }

    pub fn plan(&self, intent: &str) -> Result<WorkflowIR, PlatformError> {
        if intent.is_empty() {
            return Err(PlatformError::Planner {
                code: "EMPTY_INTENT".to_string(),
                message: "Intent cannot be empty".to_string(),
                recovery_suggestion: "Provide a valid natural language prompt or workflow spec".to_string(),
            });
        }
        let _ = &self.capability_system;
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
