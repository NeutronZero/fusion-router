use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub const WORKFLOW_IR_VERSION: u16 = 1;

/// Canonical provider-independent logical workflow (v0.13 contract 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowIR {
    pub version: u16,
    pub workflow_id: Uuid,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
    pub metadata: WorkflowMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNode {
    pub id: String,
    pub kind: WorkflowNodeKind,
    pub capability: Option<String>,
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowNodeKind {
    Task,
    Tool,
    Retrieval,
    Memory,
    Review,
    Judge,
    Security,
    Aggregation,
    Output,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowEdge {
    pub from: String,
    pub to: String,
    pub kind: WorkflowEdgeKind,
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowEdgeKind {
    Sequential,
    Parallel,
    Conditional,
    Retry,
    Merge,
    Loop,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowMetadata {
    pub policy_applied: Vec<String>,
    pub estimated_cost: f64,
    pub estimated_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ir() -> WorkflowIR {
        WorkflowIR {
            version: WORKFLOW_IR_VERSION,
            workflow_id: Uuid::new_v4(),
            nodes: vec![
                WorkflowNode {
                    id: "n1".into(),
                    kind: WorkflowNodeKind::Task,
                    capability: Some("CodeGeneration".into()),
                    config: HashMap::new(),
                },
                WorkflowNode {
                    id: "n2".into(),
                    kind: WorkflowNodeKind::Output,
                    capability: None,
                    config: HashMap::new(),
                },
            ],
            edges: vec![WorkflowEdge {
                from: "n1".into(),
                to: "n2".into(),
                kind: WorkflowEdgeKind::Sequential,
                condition: None,
            }],
            metadata: WorkflowMetadata {
                estimated_cost: 0.01,
                estimated_tokens: 500,
                ..WorkflowMetadata::default()
            },
        }
    }

    #[test]
    fn version_is_one() {
        assert_eq!(WORKFLOW_IR_VERSION, 1);
        assert_eq!(sample_ir().version, 1);
    }

    #[test]
    fn serde_round_trip_preserves_graph() {
        let ir = sample_ir();
        let json = serde_json::to_string(&ir).unwrap();
        let back: WorkflowIR = serde_json::from_str(&json).unwrap();
        assert_eq!(back.workflow_id, ir.workflow_id);
        assert_eq!(back.nodes.len(), 2);
        assert_eq!(back.nodes[0].kind, WorkflowNodeKind::Task);
        assert_eq!(back.nodes[0].capability.as_deref(), Some("CodeGeneration"));
        assert_eq!(back.edges[0].kind, WorkflowEdgeKind::Sequential);
    }

    #[test]
    fn node_rejects_model_field() {
        let node_json = r#"{"id":"n1","kind":"Task","capability":"CodeGeneration","config":{},"model":"gpt-4"}"#;
        assert!(serde_json::from_str::<WorkflowNode>(node_json).is_err());
    }

    #[test]
    fn all_node_kinds_round_trip() {
        for kind in [
            WorkflowNodeKind::Task,
            WorkflowNodeKind::Tool,
            WorkflowNodeKind::Retrieval,
            WorkflowNodeKind::Memory,
            WorkflowNodeKind::Review,
            WorkflowNodeKind::Judge,
            WorkflowNodeKind::Security,
            WorkflowNodeKind::Aggregation,
            WorkflowNodeKind::Output,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: WorkflowNodeKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn all_edge_kinds_round_trip() {
        for kind in [
            WorkflowEdgeKind::Sequential,
            WorkflowEdgeKind::Parallel,
            WorkflowEdgeKind::Conditional,
            WorkflowEdgeKind::Retry,
            WorkflowEdgeKind::Merge,
            WorkflowEdgeKind::Loop,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: WorkflowEdgeKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }
}
