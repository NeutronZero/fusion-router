use crate::edge::WorkflowEdge;
use crate::node::WorkflowNode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[cfg(test)]
use crate::version::WORKFLOW_IR_VERSION;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowIR {
    pub(crate) version: u16,
    pub(crate) workflow_id: Uuid,
    pub(crate) nodes: Vec<WorkflowNode>,
    pub(crate) edges: Vec<WorkflowEdge>,
    pub(crate) metadata: WorkflowMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowMetadata {
    pub policy_applied: Vec<String>,
    pub estimated_cost: f64,
    pub estimated_tokens: u64,
}

impl WorkflowIR {
    pub fn version(&self) -> u16 {
        self.version
    }

    pub fn workflow_id(&self) -> Uuid {
        self.workflow_id
    }

    pub fn nodes(&self) -> &[WorkflowNode] {
        &self.nodes
    }

    pub fn edges(&self) -> &[WorkflowEdge] {
        &self.edges
    }

    pub fn metadata(&self) -> &WorkflowMetadata {
        &self.metadata
    }

    pub fn validate(&self) -> crate::validate::ValidationReport {
        let mut report = crate::validate::ValidationReport::default();
        crate::validate::run_all(&self, &mut report);
        report.sort_deterministic();
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::WorkflowEdgeKind;
    use crate::node::WorkflowNodeKind;
    use std::collections::BTreeMap;

    fn sample() -> WorkflowIR {
        WorkflowIR {
            version: WORKFLOW_IR_VERSION,
            workflow_id: Uuid::new_v4(),
            nodes: vec![
                WorkflowNode {
                    id: "n1".into(),
                    kind: WorkflowNodeKind::Task,
                    capability: Some("CodeGeneration".into()),
                    config: BTreeMap::new(),
                },
                WorkflowNode {
                    id: "n2".into(),
                    kind: WorkflowNodeKind::Output,
                    capability: None,
                    config: BTreeMap::new(),
                },
            ],
            edges: vec![WorkflowEdge {
                from: "n1".into(),
                to: "n2".into(),
                kind: WorkflowEdgeKind::Sequential,
                condition: None,
            }],
            metadata: WorkflowMetadata::default(),
        }
    }

    #[test]
    fn version_is_one() {
        assert_eq!(WORKFLOW_IR_VERSION, 1);
        assert_eq!(sample().version, 1);
    }

    #[test]
    fn serde_round_trip_preserves_graph() {
        let ir = sample();
        let json = serde_json::to_string(&ir).unwrap();
        let back: WorkflowIR = serde_json::from_str(&json).unwrap();
        assert_eq!(back.workflow_id, ir.workflow_id);
        assert_eq!(back.nodes.len(), 2);
        assert_eq!(back.nodes[0].kind, WorkflowNodeKind::Task);
        assert_eq!(back.nodes[0].capability.as_deref(), Some("CodeGeneration"));
        assert_eq!(back.edges[0].kind, WorkflowEdgeKind::Sequential);
    }

    #[test]
    fn ir_rejects_provider_fields_at_serde_level() {
        let json = r#"{"version":1,"workflow_id":"00000000-0000-0000-0000-000000000000","nodes":[],"edges":[],"metadata":{"policy_applied":[],"estimated_cost":0.0,"estimated_tokens":0},"provider":"openai"}"#;
        assert!(serde_json::from_str::<WorkflowIR>(json).is_err());
    }
}
