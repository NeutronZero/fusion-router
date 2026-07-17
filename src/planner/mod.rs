use async_trait::async_trait;

mod dynamic_planner;
mod simple;
mod workflow;

#[allow(unused_imports)]
pub use dynamic_planner::{DynamicPlanner, DynamicPlannerConfig};
pub use workflow::WorkflowPlanner;

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

impl PlannerMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "dynamic" => PlannerMode::Dynamic,
            "hybrid" => PlannerMode::Hybrid,
            _ => PlannerMode::Static,
        }
    }
}
