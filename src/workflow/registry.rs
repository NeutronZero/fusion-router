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
                estimated_cost: base_cost * node_count as f64,
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
