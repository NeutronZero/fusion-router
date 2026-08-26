use crate::edge::WorkflowEdge;
use crate::node::WorkflowNode;
#[cfg(test)]
use crate::version::WORKFLOW_IR_VERSION;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    #[serde(default)]
    pub policy_version: u64,
    pub estimated_cost: fusion_core::NanoUSD,
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
        crate::validate::run_all(self, &mut report);
        report.sort_deterministic();
        report
    }
}

impl WorkflowIR {
    pub fn to_canonical_json(&self) -> Result<String, crate::error::WorkflowIrError> {
        crate::serialize::to_canonical_json(self)
    }

    pub fn from_json(s: &str) -> Result<WorkflowIR, crate::error::WorkflowIrError> {
        crate::serialize::from_json(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::WorkflowEdgeKind;
    use crate::error::WorkflowIrError;
    use crate::node::WorkflowNodeKind;
    use crate::validate::ValidationError;
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
                    selected_model: None,
                    config: BTreeMap::new(),
                },
                WorkflowNode {
                    id: "n2".into(),
                    kind: WorkflowNodeKind::Output,
                    capability: None,
                    selected_model: None,
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
        // Float money amounts deserialize via the checked NanoUSD f64 path;
        // the rejection here must come from the reserved `provider` key.
        let json = r#"{"version":1,"workflow_id":"00000000-0000-0000-0000-000000000000","nodes":[],"edges":[],"metadata":{"policy_applied":[],"estimated_cost":0.0,"estimated_tokens":0},"provider":"openai"}"#;
        let err = serde_json::from_str::<WorkflowIR>(json).unwrap_err().to_string();
        assert!(
            err.contains("unknown field") && err.contains("provider"),
            "expected unknown-field rejection, got: {err}"
        );
        // A client-supplied selected_model is rejected at the untrusted
        // intake boundary even though the structure is otherwise valid.
        let pinned = r#"{"version":1,"workflow_id":"00000000-0000-0000-0000-000000000000","nodes":[{"id":"a","kind":"Task","capability":"CodeGeneration","selected_model":"gpt-4o","config":{}}],"edges":[],"metadata":{"policy_applied":[],"estimated_cost":0,"estimated_tokens":0}}"#;
        let intake_err = WorkflowIR::from_json(pinned).unwrap_err();
        assert!(
            intake_err.to_string().contains("selected_model"),
            "expected selected_model rejection, got: {intake_err}"
        );
    }

    #[test]
    fn workflow_ir_is_deterministic() {
        let first = crate::builder::WorkflowBuilder::new()
            .with_workflow_id(Uuid::from_u128(42))
            .task("n1", "CodeGeneration")
            .unwrap()
            .task("n2", "CodeReview")
            .unwrap()
            .output("n3")
            .unwrap()
            .conditional("n1", "n2", "confidence > 0.8")
            .unwrap()
            .conditional("n1", "n2", "confidence > 0.5")
            .unwrap()
            .sequential("n2", "n3")
            .unwrap()
            .build()
            .unwrap();
        let second = crate::builder::WorkflowBuilder::new()
            .with_workflow_id(Uuid::from_u128(42))
            .output("n3")
            .unwrap()
            .task("n2", "CodeReview")
            .unwrap()
            .sequential("n2", "n3")
            .unwrap()
            .task("n1", "CodeGeneration")
            .unwrap()
            .conditional("n1", "n2", "confidence > 0.5")
            .unwrap()
            .conditional("n1", "n2", "confidence > 0.8")
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(
            first.to_canonical_json().unwrap(),
            second.to_canonical_json().unwrap()
        );
    }

    #[test]
    fn workflow_ir_round_trip_is_lossless() {
        let mut ir = sample();
        ir.metadata.estimated_cost = fusion_core::NanoUSD::from_nanos(12_345_678);
        ir.metadata.estimated_tokens = 500;
        let json = ir.to_canonical_json().unwrap();
        let back = WorkflowIR::from_json(&json).unwrap();
        assert_eq!(back.to_canonical_json().unwrap(), json);
    }

    #[test]
    fn workflow_id_stable_across_round_trip() {
        let ir = sample();
        let back = WorkflowIR::from_json(&ir.to_canonical_json().unwrap()).unwrap();
        assert_eq!(back.workflow_id, ir.workflow_id);
    }

    #[test]
    fn canonical_json_sorts_nodes_by_id() {
        let ir = crate::builder::WorkflowBuilder::new()
            .with_workflow_id(Uuid::from_u128(42))
            .task("n2", "A")
            .unwrap()
            .output("n1")
            .unwrap()
            .sequential("n2", "n1")
            .unwrap()
            .build()
            .unwrap();
        let json = ir.to_canonical_json().unwrap();
        assert!(json.find("\"n1\"").unwrap() < json.find("\"n2\"").unwrap());
    }

    #[test]
    fn from_json_rejects_wrong_version() {
        let mut ir = sample();
        ir.version = 99;
        let json = serde_json::to_string(&ir).unwrap();
        let err = WorkflowIR::from_json(&json).unwrap_err();
        assert!(matches!(
            err,
            WorkflowIrError::Validation(ValidationError::VersionMismatch(99, WORKFLOW_IR_VERSION))
        ));
    }

    #[test]
    fn from_json_rejects_invalid_workflow() {
        // Self-edge a->a must be the reason this fails (not a serde type
        // error on the float cost, which now parses).
        let json = r#"{"version":1,"workflow_id":"00000000-0000-0000-0000-000000000000","nodes":[{"id":"a","kind":"Task","capability":null,"config":{}}],"edges":[{"from":"a","to":"a","kind":"Sequential","condition":null}],"metadata":{"policy_applied":[],"estimated_cost":0.0,"estimated_tokens":0}}"#;
        let ir: WorkflowIR = serde_json::from_str(json)
            .expect("float costs deserialize; structure is well-formed");
        let report = ir.validate();
        assert!(
            !report.is_empty(),
            "self-edge must produce a validation issue"
        );
        assert!(WorkflowIR::from_json(json).is_err());
    }
}
