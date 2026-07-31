use serde::{Deserialize, Serialize};

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
