use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use uuid::Uuid;

use super::simple::SimplePlanner;
use super::Planner;
use crate::providers::ChatProvider;
use crate::types::{
    ChatCompletionRequest, ChatMessage, ComplexityLevel, EvidenceSnapshot,
    IRMetadata, IRNode, IRNodeKind, Policy, Requirements, StrategyKind, WorkflowIR,
};

pub struct DynamicPlannerConfig {
    pub max_generated_nodes: usize,
    pub generation_timeout: Duration,
    pub max_iterations: u32,
}

impl Default for DynamicPlannerConfig {
    fn default() -> Self {
        Self {
            max_generated_nodes: 20,
            generation_timeout: Duration::from_secs(10),
            max_iterations: 10,
        }
    }
}

pub struct DynamicPlanner {
    provider: Arc<dyn ChatProvider + Send + Sync>,
    fallback: SimplePlanner,
    config: DynamicPlannerConfig,
}

impl DynamicPlanner {
    pub fn new(
        provider: Arc<dyn ChatProvider + Send + Sync>,
        config: DynamicPlannerConfig,
    ) -> Self {
        Self {
            provider,
            fallback: SimplePlanner,
            config,
        }
    }

    fn build_planning_prompt(requirements: &Requirements) -> Vec<ChatMessage> {
        let system = ChatMessage {
            role: "system".to_string(),
            content: "You are a workflow planner. Given a user request, generate a JSON WorkflowIR \
                      that defines the execution plan. The workflow must be a DAG of nodes and edges. \
                      Available node types: Generate, Review, Judge, Transform, Gate, Conditional, Loop, \
                      Split, Join, Barrier. Available strategies: Single, Consensus, Reflection, Chain, \
                      Debate, ReAct. \
                      Respond ONLY with valid JSON matching this structure:\n\
                      {\n  \"nodes\": [\n    {\n      \"kind\": \"Generate\",\n      \"strategy\": \"Single\",\n      \"model\": \"model-name\"\n    }\n  ],\n  \"edges\": [\n    {\n      \"from_index\": 0,\n      \"to_index\": 1,\n      \"condition\": null\n    }\n  ]\n}\n\
                      Use from_index/to_index as 0-based indices into the nodes array.".to_string(),
        };

        let complexity_desc = match requirements.complexity {
            ComplexityLevel::Critical => "critical complexity",
            ComplexityLevel::High => "high complexity",
            ComplexityLevel::Medium => "medium complexity",
            ComplexityLevel::Low => "low complexity",
        };

        let user_msg = format!(
            "Generate a workflow plan for a request with the following characteristics:\n\
             - Intent: {:?}\n\
             - Complexity: {}\n\
             - Has files: {}\n\
             - Context window: {}",
            requirements.intent_classification,
            complexity_desc,
            requirements.has_files,
            requirements.context_window,
        );

        vec![system, ChatMessage { role: "user".to_string(), content: user_msg }]
    }

    async fn generate_ir(&self, requirements: &Requirements) -> Option<WorkflowIR> {
        let messages = Self::build_planning_prompt(requirements);

        let request = ChatCompletionRequest {
            model: "zen-7b".to_string(),
            messages,
            stream: false,
            temperature: Some(0.7),
            max_tokens: Some(2048),
            tools: None,
            files: None,
        };

        match tokio::time::timeout(
            self.config.generation_timeout,
            self.provider.chat_completion(&request),
        )
        .await
        {
            Ok(Ok(response)) => {
                let content = response.choices.first()?.message.content.trim().to_string();
                Self::parse_workflow_ir(&content, self.config.max_generated_nodes)
            }
            _ => None,
        }
    }

    fn parse_workflow_ir(content: &str, max_nodes: usize) -> Option<WorkflowIR> {
        let json: serde_json::Value = serde_json::from_str(content).ok()?;

        let nodes_val = json.get("nodes")?.as_array()?;
        if nodes_val.is_empty() || nodes_val.len() > max_nodes {
            return None;
        }

        let mut nodes = Vec::new();
        for nv in nodes_val {
            let kind_str = nv.get("kind")?.as_str()?;
            let kind = match kind_str {
                "Generate" => IRNodeKind::Generate,
                "Review" => IRNodeKind::Review,
                "Judge" => IRNodeKind::Judge,
                "Transform" => IRNodeKind::Transform,
                "Gate" => IRNodeKind::Gate,
                "Conditional" => IRNodeKind::Conditional,
                "Loop" => IRNodeKind::Loop,
                "Split" => IRNodeKind::Split,
                "Join" => IRNodeKind::Join,
                "Barrier" => IRNodeKind::Barrier,
                _ => return None,
            };

            let strategy_str = nv.get("strategy").and_then(|v| v.as_str()).unwrap_or("Single");
            let strategy = match strategy_str {
                "Single" => StrategyKind::Single,
                "Consensus" => StrategyKind::Consensus,
                "Reflection" => StrategyKind::Reflection,
                "Chain" => StrategyKind::Chain,
                "Debate" => StrategyKind::Debate,
                "ReAct" => StrategyKind::ReAct,
                "Fusion" => StrategyKind::Fusion,
                _ => StrategyKind::Single,
            };

            let model = nv.get("model").and_then(|v| v.as_str()).map(|s| s.to_string());

            nodes.push(IRNode {
                id: Uuid::new_v4(),
                kind,
                strategy,
                model,
                config: HashMap::new(),
            });
        }

        let edges_val = json.get("edges").and_then(|v| v.as_array()).map(|v| v.clone()).unwrap_or_default();
        let mut edges = Vec::new();
        for ev in &edges_val {
            let from_idx = ev.get("from_index").and_then(|v| v.as_u64())? as usize;
            let to_idx = ev.get("to_index").and_then(|v| v.as_u64())? as usize;
            if from_idx >= nodes.len() || to_idx >= nodes.len() {
                return None;
            }
            let condition = ev.get("condition").and_then(|v| v.as_str()).map(|s| s.to_string());
            edges.push(crate::types::IREdge {
                from: nodes[from_idx].id,
                to: nodes[to_idx].id,
                condition,
            });
        }

        Some(WorkflowIR {
            plan_id: Uuid::new_v4(),
            nodes,
            edges,
            metadata: IRMetadata {
                policy_applied: vec!["dynamic".to_string()],
                estimated_cost: 0.01,
                estimated_tokens: 1000,
            },
        })
    }
}

#[async_trait]
impl Planner for DynamicPlanner {
    async fn plan(
        &self,
        requirements: &Requirements,
        _policies: &[Policy],
        _evidence: Option<&EvidenceSnapshot>,
    ) -> WorkflowIR {
        match self.generate_ir(requirements).await {
            Some(ir) => ir,
            None => self.fallback.plan(requirements, _policies, _evidence).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_workflow_ir() {
        let content = r#"{
            "nodes": [
                { "kind": "Generate", "strategy": "Single", "model": "test-model" },
                { "kind": "Review", "strategy": "Single", "model": "test-model" }
            ],
            "edges": [
                { "from_index": 0, "to_index": 1, "condition": null }
            ]
        }"#;

        let ir = DynamicPlanner::parse_workflow_ir(content, 20);
        assert!(ir.is_some(), "Should parse valid WorkflowIR JSON");
        let ir = ir.unwrap();
        assert_eq!(ir.nodes.len(), 2);
        assert_eq!(ir.edges.len(), 1);
    }

    #[test]
    fn test_parse_exceeds_max_nodes() {
        let content = r#"{
            "nodes": [
                { "kind": "Generate", "strategy": "Single", "model": "m" },
                { "kind": "Generate", "strategy": "Single", "model": "m" },
                { "kind": "Generate", "strategy": "Single", "model": "m" }
            ],
            "edges": []
        }"#;

        let ir = DynamicPlanner::parse_workflow_ir(content, 2);
        assert!(ir.is_none(), "Should reject IR exceeding max_nodes");
    }

    #[test]
    fn test_parse_invalid_node_kind() {
        let content = r#"{
            "nodes": [
                { "kind": "InvalidKind", "strategy": "Single", "model": "m" }
            ],
            "edges": []
        }"#;

        let ir = DynamicPlanner::parse_workflow_ir(content, 20);
        assert!(ir.is_none(), "Should reject unknown node kind");
    }

    #[test]
    fn test_parse_out_of_bounds_edge() {
        let content = r#"{
            "nodes": [
                { "kind": "Generate", "strategy": "Single", "model": "m" }
            ],
            "edges": [
                { "from_index": 0, "to_index": 5, "condition": null }
            ]
        }"#;

        let ir = DynamicPlanner::parse_workflow_ir(content, 20);
        assert!(ir.is_none(), "Should reject out-of-bounds edge index");
    }
}
