use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNode {
    pub(crate) id: String,
    pub(crate) kind: WorkflowNodeKind,
    pub(crate) capability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) selected_model: Option<String>,
    pub(crate) config: BTreeMap<String, serde_json::Value>,
}

impl WorkflowNode {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn kind(&self) -> WorkflowNodeKind {
        self.kind
    }

    pub fn capability(&self) -> Option<&str> {
        self.capability.as_deref()
    }

    pub fn selected_model(&self) -> Option<&str> {
        self.selected_model.as_deref()
    }

    pub fn config(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.config
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn node_rejects_model_field() {
        let json = r#"{"id":"n1","kind":"Task","capability":"CodeGeneration","config":{},"model":"gpt-4"}"#;
        assert!(serde_json::from_str::<WorkflowNode>(json).is_err());
    }

    #[test]
    fn config_keys_serialize_sorted() {
        let mut config = BTreeMap::new();
        config.insert("zebra".into(), serde_json::Value::Bool(true));
        config.insert("alpha".into(), serde_json::Value::Bool(false));
        let node = WorkflowNode {
            id: "n1".into(),
            kind: WorkflowNodeKind::Task,
            capability: None,
            selected_model: None,
            config,
        };
        let json = serde_json::to_string(&node).unwrap();
        assert!(json.find("\"alpha\"").unwrap() < json.find("\"zebra\"").unwrap());
    }
}
