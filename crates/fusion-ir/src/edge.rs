use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowEdge {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) kind: WorkflowEdgeKind,
    pub(crate) condition: Option<String>,
}

impl WorkflowEdge {
    pub fn from(&self) -> &str {
        &self.from
    }

    pub fn to(&self) -> &str {
        &self.to
    }

    pub fn kind(&self) -> WorkflowEdgeKind {
        self.kind
    }

    pub fn condition(&self) -> Option<&str> {
        self.condition.as_deref()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
