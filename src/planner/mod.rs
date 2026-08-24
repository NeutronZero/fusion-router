use async_trait::async_trait;

mod intent_planner;

pub use intent_planner::IntentPlanner;

use crate::types::{EvidenceSnapshot, Policy, Requirements, WorkflowIR};

/// Failure raised by planning. Surfaced to clients as a retryable 503.
#[derive(Debug)]
pub struct PlannerFailure(pub String);

impl std::fmt::Display for PlannerFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "planning failed: {}", self.0)
    }
}

impl std::error::Error for PlannerFailure {}

#[async_trait]
pub trait Planner: Send + Sync {
    async fn plan(
        &self,
        requirements: &Requirements,
        policies: &[Policy],
        evidence: Option<&EvidenceSnapshot>,
    ) -> Result<WorkflowIR, PlannerFailure>;

    async fn plan_with_policy_version(
        &self,
        requirements: &Requirements,
        policies: &[Policy],
        evidence: Option<&EvidenceSnapshot>,
        _policy_version: u64,
    ) -> Result<WorkflowIR, PlannerFailure> {
        self.plan(requirements, policies, evidence).await
    }
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
