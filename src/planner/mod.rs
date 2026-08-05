use async_trait::async_trait;

mod dynamic_planner;
mod intent_planner;
mod simple;
mod workflow;
pub mod resolver;

pub use intent_planner::IntentPlanner;
pub use fusion_planner::{PlannerService, ExecutionIntent, PlannerKind, PlannerContract};

use crate::types::{EvidenceSnapshot, Policy, Requirements, WorkflowIR};

#[async_trait]
pub trait Planner: Send + Sync {
    async fn plan(
        &self,
        requirements: &Requirements,
        policies: &[Policy],
        evidence: Option<&EvidenceSnapshot>,
    ) -> WorkflowIR;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlannerMode {
    Static,
    Dynamic,
    Hybrid,
}

impl std::str::FromStr for PlannerMode {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "dynamic" => PlannerMode::Dynamic,
            "hybrid" => PlannerMode::Hybrid,
            _ => PlannerMode::Static,
        })
    }
}
