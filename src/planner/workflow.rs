use async_trait::async_trait;
use std::sync::Arc;

use super::intent_planner::IntentPlanner;
use super::{Planner, PlannerMode};
use crate::types::{EvidenceSnapshot, ModelCatalog, Policy, Requirements, WorkflowIR};
use crate::workflow::WorkflowRegistry;

#[allow(dead_code)]
pub struct WorkflowPlanner {
    registry: Arc<WorkflowRegistry>,
    fallback: IntentPlanner,
    mode: PlannerMode,
}

#[allow(dead_code)]
impl WorkflowPlanner {
    pub fn new(registry: Arc<WorkflowRegistry>) -> Self {
        Self {
            registry,
            fallback: IntentPlanner::new(ModelCatalog::default()),
            mode: PlannerMode::Static,
        }
    }

    pub fn with_mode(mut self, mode: PlannerMode) -> Self {
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
            PlannerMode::Static | PlannerMode::Dynamic | PlannerMode::Hybrid => {
                if let Some(def) = self.registry.select(requirements) {
                    def.instantiate(requirements)
                } else {
                    self.fallback.plan(requirements, policies, evidence).await
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ComplexityLevel, Intent, IRNodeKind};
    use crate::workflow::WorkflowDefinition;

    fn make_requirements(intent: Intent, complexity: ComplexityLevel) -> Requirements {
        Requirements {
            intent_classification: intent,
            complexity,
            has_files: false,
            context_window: 4096,
            original_text: String::new(),
            execution_intent: None,
            output_preferences: None,
            model_requirements: None,
            requested_strategy: None,
            requested_model: None,
        }
    }

    fn make_registry_with_def() -> Arc<WorkflowRegistry> {
        let yaml = r#"
name: test-workflow
description: Test workflow
required_intents: ["Code"]
min_complexity: 0
node_templates:
  - kind: Generate
    strategy: Single
    model: test-model
  - kind: Review
    strategy: Single
    model: test-model
edges:
  - from: 0
    to: 1
"#;
        let def: WorkflowDefinition = serde_yaml::from_str(yaml).unwrap();
        let mut registry = WorkflowRegistry::new();
        registry.register(def);
        Arc::new(registry)
    }

    #[tokio::test]
    async fn test_workflow_planner_static_mode() {
        let registry = make_registry_with_def();
        let planner = WorkflowPlanner::new(registry);
        let reqs = make_requirements(Intent::Code, ComplexityLevel::Medium);
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 2);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[1].kind, IRNodeKind::Review);
        assert_eq!(ir.edges.len(), 1);
    }

    #[tokio::test]
    async fn test_workflow_planner_static_fallback() {
        let registry = Arc::new(WorkflowRegistry::new());
        let planner = WorkflowPlanner::new(registry);
        let reqs = make_requirements(Intent::Code, ComplexityLevel::Medium);
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 1);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
    }

    #[tokio::test]
    async fn test_workflow_planner_hybrid_mode() {
        let registry = make_registry_with_def();
        let planner = WorkflowPlanner::new(registry).with_mode(PlannerMode::Hybrid);
        let reqs = make_requirements(Intent::Code, ComplexityLevel::Medium);
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 2);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[1].kind, IRNodeKind::Review);
    }
}
