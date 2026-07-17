use async_trait::async_trait;
use std::sync::Arc;

use super::simple::SimplePlanner;
use super::Planner;
use crate::types::{EvidenceSnapshot, Policy, Requirements, WorkflowIR};
use crate::workflow::WorkflowRegistry;

pub struct WorkflowPlanner {
    registry: Arc<WorkflowRegistry>,
    fallback: SimplePlanner,
}

impl WorkflowPlanner {
    pub fn new(registry: Arc<WorkflowRegistry>) -> Self {
        Self {
            registry,
            fallback: SimplePlanner,
        }
    }
}

#[async_trait]
impl Planner for WorkflowPlanner {
    async fn plan(
        &self,
        requirements: &Requirements,
        _policies: &[Policy],
        _evidence: Option<&EvidenceSnapshot>,
    ) -> WorkflowIR {
        if let Some(def) = self.registry.select(requirements) {
            def.instantiate(requirements)
        } else {
            self.fallback.plan(requirements, _policies, _evidence).await
        }
    }
}
