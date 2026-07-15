use async_trait::async_trait;

mod simple;

pub use simple::SimplePlanner;

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
