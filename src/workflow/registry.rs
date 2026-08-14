use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{Intent, IRNode, IRNodeKind, Requirements, StrategyKind, WorkflowIR};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub name: String,
    pub description: String,
    pub required_intents: Vec<Intent>,
    pub min_complexity: u8,
    #[serde(default)]
    pub requires_files: bool,
    pub node_templates: Vec<NodeTemplate>,
    #[serde(default)]
    pub edges: Vec<EdgeTemplate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTemplate {
    pub kind: IRNodeKind,
    pub strategy: StrategyKind,
    pub model: Option<String>,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeTemplate {
    pub from: usize,
    pub to: usize,
    pub condition: Option<String>,
}

impl WorkflowDefinition {
    pub fn can_handle(&self, reqs: &Requirements) -> bool {
        if !self.required_intents.is_empty() && !self.required_intents.contains(&reqs.intent_classification) {
            return false;
        }
        if self.requires_files && !reqs.has_files {
            return false;
        }
        true
    }

    pub fn instantiate(&self, reqs: &Requirements) -> WorkflowIR {
        let plan_id = Uuid::new_v4();
        let mut nodes = Vec::new();

        for tmpl in &self.node_templates {
            let model = tmpl.model.clone().or_else(|| {
                if matches!(tmpl.kind, IRNodeKind::Generate | IRNodeKind::Review | IRNodeKind::Judge) {
                    Some("claude-sonnet-4-20250514".to_string())
                } else {
                    None
                }
            });

            nodes.push(IRNode {
                id: Uuid::new_v4(),
                kind: tmpl.kind.clone(),
                strategy: tmpl.strategy.clone(),
                model,
                config: tmpl.config.clone(),
            });
        }

        let edges = self.edges.iter().map(|et| {
            let from_id = nodes.get(et.from).map(|n| n.id).unwrap_or_default();
            let to_id = nodes.get(et.to).map(|n| n.id).unwrap_or_default();
            crate::types::IREdge {
                from: from_id,
                to: to_id,
                condition: et.condition.clone(),
            }
        }).collect();

        let node_count = nodes.len();

        let base_cost = match reqs.complexity {
            crate::types::ComplexityLevel::Low => 0.01,
            crate::types::ComplexityLevel::Medium => 0.05,
            crate::types::ComplexityLevel::High => 0.10,
            crate::types::ComplexityLevel::Critical => 0.25,
        };

        WorkflowIR {
            plan_id,
            nodes,
            edges,
            metadata: crate::types::IRMetadata {
                policy_applied: vec!["workflow_definition".to_string()],
                policy_version: 0,
                estimated_cost: crate::types::NanoUSD::from_nanos((base_cost * 1_000_000_000.0 * node_count as f64) as u64),
                estimated_tokens: 1000 * node_count as u64,
            },
        }
    }
}

#[derive(Debug, Default)]
pub struct WorkflowRegistry {
    definitions: HashMap<String, WorkflowDefinition>,
}

impl WorkflowRegistry {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
        }
    }

    pub fn register(&mut self, def: WorkflowDefinition) {
        self.definitions.insert(def.name.clone(), def);
    }

    pub fn get(&self, name: &str) -> Option<&WorkflowDefinition> {
        self.definitions.get(name)
    }

    pub fn list(&self) -> Vec<&WorkflowDefinition> {
        self.definitions.values().collect()
    }

    pub fn load_dir<P: AsRef<Path>>(&mut self, dir: P) -> anyhow::Result<usize> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Ok(0);
        }

        let mut count = 0;
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "yaml" || e == "yml") {
                let content = std::fs::read_to_string(&path)?;
                let def: WorkflowDefinition = serde_yaml::from_str(&content)?;
                self.register(def);
                count += 1;
            }
        }

        Ok(count)
    }

    pub fn select(&self, reqs: &Requirements) -> Option<&WorkflowDefinition> {
        self.definitions.values().find(|def| def.can_handle(reqs))
    }

    pub fn len(&self) -> usize {
        self.definitions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ComplexityLevel, Intent, Requirements};

    fn reqs(intent: Intent, has_files: bool, complexity: ComplexityLevel) -> Requirements {
        Requirements {
            intent_classification: intent,
            complexity,
            has_files,
            context_window: 8192,
            original_text: "test".into(),
            execution_intent: None,
            output_preferences: None,
            model_requirements: None,
            requested_strategy: None,
        }
    }

    fn def(name: &str, intents: Vec<Intent>, requires_files: bool) -> WorkflowDefinition {
        WorkflowDefinition {
            name: name.into(),
            description: String::new(),
            required_intents: intents,
            min_complexity: 0,
            requires_files,
            node_templates: vec![NodeTemplate {
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: None,
                config: HashMap::new(),
            }],
            edges: vec![],
        }
    }

    #[test]
    fn test_can_handle_matching_intent() {
        let wf = def("wf-code", vec![Intent::Code], false);
        assert!(wf.can_handle(&reqs(Intent::Code, false, ComplexityLevel::Medium)));
    }

    #[test]
    fn test_can_handle_rejects_wrong_intent() {
        let wf = def("wf-code", vec![Intent::Code], false);
        assert!(!wf.can_handle(&reqs(Intent::Debug, false, ComplexityLevel::Medium)));
    }

    #[test]
    fn test_can_handle_unrestricted_intent() {
        let wf = def("wf-any", vec![], false);
        assert!(wf.can_handle(&reqs(Intent::Creative, false, ComplexityLevel::Low)));
    }

    #[test]
    fn test_can_handle_requires_files() {
        let wf = def("wf-files", vec![], true);
        assert!(wf.can_handle(&reqs(Intent::Code, true, ComplexityLevel::Medium)));
        assert!(!wf.can_handle(&reqs(Intent::Code, false, ComplexityLevel::Medium)));
    }

    #[test]
    fn test_select_returns_matching_workflow() {
        let mut registry = WorkflowRegistry::new();
        registry.register(def("wf-debug", vec![Intent::Debug], false));
        registry.register(def("wf-code", vec![Intent::Code], false));

        let selected = registry.select(&reqs(Intent::Code, false, ComplexityLevel::High)).unwrap();
        assert_eq!(selected.name, "wf-code");
    }

    #[test]
    fn test_select_returns_none_when_no_match() {
        let mut registry = WorkflowRegistry::new();
        registry.register(def("wf-code", vec![Intent::Code], false));

        assert!(registry.select(&reqs(Intent::Architecture, false, ComplexityLevel::Low)).is_none());
    }

    #[test]
    fn test_instantiate_creates_ir_with_default_model() {
        let wf = def("wf-gen", vec![], false);
        let ir = wf.instantiate(&reqs(Intent::Code, false, ComplexityLevel::Low));

        assert_eq!(ir.nodes.len(), 1);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[0].model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert_eq!(ir.metadata.estimated_tokens, 1000);
        assert_eq!(ir.metadata.estimated_cost, crate::types::NanoUSD::from_nanos(10_000_000));
    }

    #[test]
    fn test_instantiate_respects_explicit_model() {
        let mut wf = def("wf-gen", vec![], false);
        wf.node_templates[0].model = Some("gpt-4o".into());
        let ir = wf.instantiate(&reqs(Intent::Code, false, ComplexityLevel::High));

        assert_eq!(ir.nodes[0].model.as_deref(), Some("gpt-4o"));
        assert_eq!(ir.metadata.estimated_cost, crate::types::NanoUSD::from_nanos(100_000_000));
    }

    #[test]
    fn test_instantiate_wires_edges_by_index() {
        let wf = WorkflowDefinition {
            name: "wf-edges".into(),
            description: String::new(),
            required_intents: vec![],
            min_complexity: 0,
            requires_files: false,
            node_templates: vec![
                NodeTemplate {
                    kind: IRNodeKind::Generate,
                    strategy: StrategyKind::Single,
                    model: None,
                    config: HashMap::new(),
                },
                NodeTemplate {
                    kind: IRNodeKind::Judge,
                    strategy: StrategyKind::Single,
                    model: None,
                    config: HashMap::new(),
                },
            ],
            edges: vec![EdgeTemplate {
                from: 0,
                to: 1,
                condition: None,
            }],
        };

        let ir = wf.instantiate(&reqs(Intent::Code, false, ComplexityLevel::Medium));

        assert_eq!(ir.edges.len(), 1);
        assert_eq!(ir.edges[0].from, ir.nodes[0].id);
        assert_eq!(ir.edges[0].to, ir.nodes[1].id);
    }
}
