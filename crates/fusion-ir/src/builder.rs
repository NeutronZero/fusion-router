use crate::edge::{WorkflowEdge, WorkflowEdgeKind};
use crate::node::{WorkflowNode, WorkflowNodeKind};
use crate::validate::ValidationError;
use crate::version::WORKFLOW_IR_VERSION;
use crate::workflow::{WorkflowIR, WorkflowMetadata};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

#[derive(Debug)]
pub struct WorkflowBuilder {
    workflow_id: Option<Uuid>,
    nodes: Vec<WorkflowNode>,
    edges: Vec<WorkflowEdge>,
    metadata: WorkflowMetadata,
    seen: HashSet<String>,
}

impl WorkflowBuilder {
    pub fn new() -> Self {
        Self {
            workflow_id: None,
            nodes: Vec::new(),
            edges: Vec::new(),
            metadata: WorkflowMetadata::default(),
            seen: HashSet::new(),
        }
    }

    pub fn with_workflow_id(mut self, id: Uuid) -> Self {
        self.workflow_id = Some(id);
        self
    }

    pub fn metadata(mut self, metadata: WorkflowMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn add_node(mut self, id: &str, kind: WorkflowNodeKind, capability: Option<&str>) -> Result<Self, ValidationError> {
        if !self.seen.insert(id.to_string()) {
            return Err(ValidationError::DuplicateNodeId(id.to_string()));
        }
        self.nodes.push(WorkflowNode {
            id: id.to_string(),
            kind,
            capability: capability.map(String::from),
            config: BTreeMap::new(),
        });
        Ok(self)
    }

    pub fn add_node_with_config(
        mut self,
        id: &str,
        kind: WorkflowNodeKind,
        capability: Option<&str>,
        config: BTreeMap<String, serde_json::Value>,
    ) -> Result<Self, ValidationError> {
        if !self.seen.insert(id.to_string()) {
            return Err(ValidationError::DuplicateNodeId(id.to_string()));
        }
        self.nodes.push(WorkflowNode {
            id: id.to_string(),
            kind,
            capability: capability.map(String::from),
            config,
        });
        Ok(self)
    }

    pub fn with_config(mut self, id: &str, config: BTreeMap<String, serde_json::Value>) -> Result<Self, ValidationError> {
        let node = self
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| ValidationError::UnknownNodeRef(id.to_string()))?;
        node.config = config;
        Ok(self)
    }

    pub fn task(self, id: &str, capability: &str) -> Result<Self, ValidationError> {
        self.add_node(id, WorkflowNodeKind::Task, Some(capability))
    }

    pub fn tool(self, id: &str, capability: &str) -> Result<Self, ValidationError> {
        self.add_node(id, WorkflowNodeKind::Tool, Some(capability))
    }

    pub fn retrieval(self, id: &str, capability: &str) -> Result<Self, ValidationError> {
        self.add_node(id, WorkflowNodeKind::Retrieval, Some(capability))
    }

    pub fn memory(self, id: &str, capability: &str) -> Result<Self, ValidationError> {
        self.add_node(id, WorkflowNodeKind::Memory, Some(capability))
    }

    pub fn review(self, id: &str, capability: &str) -> Result<Self, ValidationError> {
        self.add_node(id, WorkflowNodeKind::Review, Some(capability))
    }

    pub fn judge(self, id: &str, capability: &str) -> Result<Self, ValidationError> {
        self.add_node(id, WorkflowNodeKind::Judge, Some(capability))
    }

    pub fn security(self, id: &str, capability: &str) -> Result<Self, ValidationError> {
        self.add_node(id, WorkflowNodeKind::Security, Some(capability))
    }

    pub fn aggregation(self, id: &str, capability: &str) -> Result<Self, ValidationError> {
        self.add_node(id, WorkflowNodeKind::Aggregation, Some(capability))
    }

    pub fn output(self, id: &str) -> Result<Self, ValidationError> {
        self.add_node(id, WorkflowNodeKind::Output, None)
    }

    pub fn edge(mut self, from: &str, to: &str, kind: WorkflowEdgeKind, condition: Option<String>) -> Result<Self, ValidationError> {
        for ref_id in [from, to] {
            if !self.seen.contains(ref_id) {
                return Err(ValidationError::UnknownNodeRef(ref_id.to_string()));
            }
        }
        self.edges.push(WorkflowEdge {
            from: from.to_string(),
            to: to.to_string(),
            kind,
            condition,
        });
        Ok(self)
    }

    pub fn sequential(self, from: &str, to: &str) -> Result<Self, ValidationError> {
        self.edge(from, to, WorkflowEdgeKind::Sequential, None)
    }

    pub fn parallel(self, from: &str, to: &str) -> Result<Self, ValidationError> {
        self.edge(from, to, WorkflowEdgeKind::Parallel, None)
    }

    pub fn conditional(self, from: &str, to: &str, condition: &str) -> Result<Self, ValidationError> {
        self.edge(from, to, WorkflowEdgeKind::Conditional, Some(condition.to_string()))
    }

    pub fn retry(self, from: &str, to: &str) -> Result<Self, ValidationError> {
        self.edge(from, to, WorkflowEdgeKind::Retry, None)
    }

    pub fn merge(self, from: &str, to: &str) -> Result<Self, ValidationError> {
        self.edge(from, to, WorkflowEdgeKind::Merge, None)
    }

    pub fn loop_edge(self, from: &str, to: &str) -> Result<Self, ValidationError> {
        self.edge(from, to, WorkflowEdgeKind::Loop, None)
    }

    pub fn build(self) -> Result<WorkflowIR, ValidationError> {
        let ir = WorkflowIR {
            version: WORKFLOW_IR_VERSION,
            workflow_id: self.workflow_id.unwrap_or_else(Uuid::new_v4),
            nodes: self.nodes,
            edges: self.edges,
            metadata: self.metadata,
        };
        match ir.validate().first_error() {
            Some(e) => Err(e.clone()),
            None => Ok(ir),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_node_chain() -> Result<WorkflowIR, ValidationError> {
        WorkflowBuilder::new()
            .task("n1", "CodeGeneration")?
            .output("n2")?
            .sequential("n1", "n2")?
            .build()
    }

    #[test]
    fn builds_valid_workflow() {
        let ir = two_node_chain().unwrap();
        assert_eq!(ir.version, WORKFLOW_IR_VERSION);
        assert_eq!(ir.nodes.len(), 2);
        assert_eq!(ir.nodes[0].kind, WorkflowNodeKind::Task);
        assert_eq!(ir.nodes[0].capability.as_deref(), Some("CodeGeneration"));
        assert_eq!(ir.edges.len(), 1);
        assert!(ir.validate().is_empty());
    }

    #[test]
    fn duplicate_node_id_fails_at_call() {
        let err = WorkflowBuilder::new().task("n1", "A").and_then(|b| b.task("n1", "B")).unwrap_err();
        assert_eq!(err, ValidationError::DuplicateNodeId("n1".into()));
    }

    #[test]
    fn unknown_edge_reference_fails_at_call() {
        let err = WorkflowBuilder::new().sequential("a", "b").unwrap_err();
        assert_eq!(err, ValidationError::UnknownNodeRef("a".into()));
    }

    #[test]
    fn build_runs_full_structural_validation() -> Result<(), ValidationError> {
        let err = WorkflowBuilder::new().task("a", "A")?.loop_edge("a", "a")?.build().unwrap_err();
        assert_eq!(err, ValidationError::MissingRoot);
        Ok(())
    }

    #[test]
    fn with_workflow_id_and_metadata_are_preserved() -> Result<(), ValidationError> {
        let id = Uuid::new_v4();
        let ir = WorkflowBuilder::new()
            .with_workflow_id(id)
            .metadata(WorkflowMetadata {
                estimated_tokens: 100,
                ..WorkflowMetadata::default()
            })
            .task("n1", "A")?
            .output("n2")?
            .sequential("n1", "n2")?
            .build()?;
        assert_eq!(ir.workflow_id, id);
        assert_eq!(ir.metadata.estimated_tokens, 100);
        Ok(())
    }

    #[test]
    fn with_config_sets_node_config() -> Result<(), ValidationError> {
        let mut config = BTreeMap::new();
        config.insert("goal".into(), serde_json::Value::String("payments".into()));
        let ir = WorkflowBuilder::new()
            .task("n1", "A")?
            .with_config("n1", config)?
            .output("n2")?
            .sequential("n1", "n2")?
            .build()?;
        assert_eq!(ir.nodes[0].config.get("goal"), Some(&serde_json::Value::String("payments".into())));
        Ok(())
    }

    #[test]
    fn conditional_and_loop_edges_are_supported() -> Result<(), ValidationError> {
        let ir = WorkflowBuilder::new()
            .task("n1", "A")?
            .task("n2", "B")?
            .review("n3", "C")?
            .output("n4")?
            .conditional("n1", "n2", "confidence > 0.8")?
            .sequential("n2", "n3")?
            .sequential("n3", "n4")?
            .loop_edge("n3", "n2")?
            .build()?;
        assert_eq!(ir.edges.len(), 4);
        assert!(ir.validate().is_empty());
        Ok(())
    }
}
