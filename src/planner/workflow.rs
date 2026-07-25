use async_trait::async_trait;
use std::sync::Arc;

use super::dynamic_planner::DynamicPlanner;
use super::simple::SimplePlanner;
use super::{Planner, PlannerMode};
use crate::types::{EvidenceSnapshot, Policy, Requirements, WorkflowIR};
use crate::workflow::WorkflowRegistry;

#[allow(dead_code)]
pub struct WorkflowPlanner {
    registry: Arc<WorkflowRegistry>,
    dynamic: Option<Arc<DynamicPlanner>>,
    fallback: SimplePlanner,
    mode: PlannerMode,
}

#[allow(dead_code)]
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
                    if ir.nodes.len() > 1 || ir.nodes.first().is_some_and(|n| n.kind != crate::types::IRNodeKind::Generate) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::dynamic_planner::DynamicPlannerConfig;
    use crate::providers::ChatProvider;
    use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice, ComplexityLevel, Intent, IRNodeKind};
    use crate::workflow::WorkflowDefinition;

    struct MockPlannerProvider {
        response: String,
    }

    #[async_trait]
    impl ChatProvider for MockPlannerProvider {
        fn name(&self) -> &str {
            "mock-planner"
        }

        async fn chat_completion(
            &self,
            _request: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                id: "test-id".into(),
                object: "chat.completion".into(),
                created: 0,
                model: "test-model".into(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: self.response.clone(),
                    },
                    finish_reason: "stop".into(),
                }],
                usage: None,
            })
        }
    }

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
    async fn test_workflow_planner_dynamic_mode() {
        let non_trivial_json = r#"{
            "nodes": [
                {"kind": "Generate", "strategy": "Single", "model": "m1"},
                {"kind": "Review", "strategy": "Single", "model": "m2"}
            ],
            "edges": [
                {"from_index": 0, "to_index": 1, "condition": null}
            ]
        }"#;

        let registry = Arc::new(WorkflowRegistry::new());
        let dp = Arc::new(DynamicPlanner::new(
            Arc::new(MockPlannerProvider { response: non_trivial_json.to_string() }),
            DynamicPlannerConfig::default(),
        ));
        let planner = WorkflowPlanner::new(registry).with_dynamic(dp, PlannerMode::Dynamic);
        let reqs = make_requirements(Intent::Code, ComplexityLevel::Medium);
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 2);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[1].kind, IRNodeKind::Review);
    }

    #[tokio::test]
    async fn test_workflow_planner_dynamic_fallback() {
        let registry = Arc::new(WorkflowRegistry::new());
        let planner = WorkflowPlanner {
            registry,
            dynamic: None,
            fallback: SimplePlanner,
            mode: PlannerMode::Dynamic,
        };
        let reqs = make_requirements(Intent::Code, ComplexityLevel::Medium);
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 1);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
    }

    #[tokio::test]
    async fn test_workflow_planner_hybrid_returns_dynamic() {
        let non_trivial_json = r#"{
            "nodes": [
                {"kind": "Generate", "strategy": "Single", "model": "m1"},
                {"kind": "Review", "strategy": "Single", "model": "m2"}
            ],
            "edges": [
                {"from_index": 0, "to_index": 1, "condition": null}
            ]
        }"#;

        let registry = Arc::new(WorkflowRegistry::new());
        let dp = Arc::new(DynamicPlanner::new(
            Arc::new(MockPlannerProvider { response: non_trivial_json.to_string() }),
            DynamicPlannerConfig::default(),
        ));
        let planner = WorkflowPlanner::new(registry).with_dynamic(dp, PlannerMode::Hybrid);
        let reqs = make_requirements(Intent::Code, ComplexityLevel::Medium);
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 2);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[1].kind, IRNodeKind::Review);
    }

    #[tokio::test]
    async fn test_workflow_planner_hybrid_falls_through() {
        let trivial_json = r#"{
            "nodes": [
                {"kind": "Generate", "strategy": "Single", "model": "m1"}
            ],
            "edges": []
        }"#;

        let registry = make_registry_with_def();
        let dp = Arc::new(DynamicPlanner::new(
            Arc::new(MockPlannerProvider { response: trivial_json.to_string() }),
            DynamicPlannerConfig::default(),
        ));
        let planner = WorkflowPlanner::new(registry).with_dynamic(dp, PlannerMode::Hybrid);
        let reqs = make_requirements(Intent::Code, ComplexityLevel::Medium);
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 2);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[1].kind, IRNodeKind::Review);
    }
}
