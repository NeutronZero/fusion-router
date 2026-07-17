use async_trait::async_trait;
use std::sync::Arc;

use super::dynamic_planner::DynamicPlanner;
use super::simple::SimplePlanner;
use super::{Planner, PlannerMode};
use crate::types::{EvidenceSnapshot, Policy, Requirements, WorkflowIR};
use crate::workflow::WorkflowRegistry;

pub struct WorkflowPlanner {
    registry: Arc<WorkflowRegistry>,
    dynamic: Option<Arc<DynamicPlanner>>,
    fallback: SimplePlanner,
    mode: PlannerMode,
}

impl WorkflowPlanner {
    pub fn new(registry: Arc<WorkflowRegistry>) -> Self {
        Self {
            registry,
            dynamic: None,
            fallback: SimplePlanner,
            mode: PlannerMode::Static,
        }
    }

    pub fn with_dynamic(mut self, dynamic: Arc<DynamicPlanner>, mode: PlannerMode) -> Self {
        self.dynamic = Some(dynamic);
        self.mode = mode;
        self
    }
}

#[async_trait]
impl Planner for WorkflowPlanner {
    async fn plan(
        &self,
        requirements: &Requirements,
        policies: &[Policy],
        evidence: Option<&EvidenceSnapshot>,
    ) -> WorkflowIR {
        match self.mode {
            PlannerMode::Static => {
                if let Some(def) = self.registry.select(requirements) {
                    def.instantiate(requirements)
                } else {
                    self.fallback.plan(requirements, policies, evidence).await
                }
            }
            PlannerMode::Dynamic => {
                if let Some(ref dp) = self.dynamic {
                    dp.plan(requirements, policies, evidence).await
                } else {
                    self.fallback.plan(requirements, policies, evidence).await
                }
            }
            PlannerMode::Hybrid => {
                if let Some(ref dp) = self.dynamic {
                    let ir = dp.plan(requirements, policies, evidence).await;
                    if ir.nodes.len() > 1 || ir.nodes.first().map_or(false, |n| n.kind != crate::types::IRNodeKind::Generate) {
                        return ir;
                    }
                }
                if let Some(def) = self.registry.select(requirements) {
                    def.instantiate(requirements)
                } else {
                    self.fallback.plan(requirements, policies, evidence).await
                }
            }
        }
    }
}
