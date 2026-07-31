use crate::workflow::WorkflowIR;
use thiserror::Error;
#[cfg(test)]
use crate::edge::{WorkflowEdge, WorkflowEdgeKind};
#[cfg(test)]
use crate::node::{WorkflowNode, WorkflowNodeKind};
#[cfg(test)]
use crate::version::WORKFLOW_IR_VERSION;
#[cfg(test)]
use crate::workflow::WorkflowMetadata;
#[cfg(test)]
use std::collections::BTreeMap;
#[cfg(test)]
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("duplicate node id `{0}`")]
    DuplicateNodeId(String),
    #[error("edge references unknown node `{0}`")]
    UnknownNodeRef(String),
    #[error("node `{0}` is unreachable")]
    UnreachableNode(String),
    #[error("graph has no root node")]
    MissingRoot,
    #[error("edge `{from}` -> `{to}` may not form a cycle (only Loop edges may cycle)")]
    IllegalCycle { from: String, to: String },
    #[error("conditional edge `{from}` -> `{to}` requires a condition")]
    MissingCondition { from: String, to: String },
    #[error("retry edge source `{0}` is not retryable")]
    NotRetryable(String),
    #[error("merge target `{0}` requires at least two incoming edges")]
    MergeArity(String),
    #[error("output node `{0}` may not have outgoing edges")]
    OutputOutgoing(String),
    #[error("provider-identifying field `{0}` is reserved by the architecture")]
    ProviderField(String),
    #[error("workflow version {0} is not supported (expected {1})")]
    VersionMismatch(u16, u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub node: Option<String>,
    pub edge: Option<String>,
    pub error: ValidationError,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidationReport {
    issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn issues(&self) -> &[ValidationIssue] {
        &self.issues
    }

    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn len(&self) -> usize {
        self.issues.len()
    }

    pub fn first_error(&self) -> Option<&ValidationError> {
        self.issues.first().map(|i| &i.error)
    }

    pub(crate) fn push(&mut self, issue: ValidationIssue) {
        self.issues.push(issue);
    }

    pub(crate) fn sort_deterministic(&mut self) {
        self.issues.sort_by_key(|i| {
            (
                i.node.clone().unwrap_or_default(),
                i.edge.clone().unwrap_or_default(),
                format!("{}", i.error),
            )
        });
    }
}

fn structural_checks(ir: &WorkflowIR, report: &mut ValidationReport) {
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for n in &ir.nodes {
        if !seen.insert(n.id.as_str()) {
            report.push(ValidationIssue {
                node: Some(n.id.clone()),
                edge: None,
                error: ValidationError::DuplicateNodeId(n.id.clone()),
            });
        }
    }

    for e in &ir.edges {
        if !seen.contains(e.from.as_str()) {
            report.push(ValidationIssue {
                node: None,
                edge: Some(format!("{} -> {}", e.from, e.to)),
                error: ValidationError::UnknownNodeRef(e.from.clone()),
            });
        }
        if !seen.contains(e.to.as_str()) {
            report.push(ValidationIssue {
                node: None,
                edge: Some(format!("{} -> {}", e.from, e.to)),
                error: ValidationError::UnknownNodeRef(e.to.clone()),
            });
        }
    }

    let mut incoming: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for e in &ir.edges {
        if seen.contains(e.to.as_str()) {
            *incoming.entry(e.to.as_str()).or_insert(0) += 1;
        }
    }
    let roots: Vec<&str> = ir.nodes.iter().map(|n| n.id.as_str()).filter(|id| incoming.get(id).copied().unwrap_or(0) == 0).collect();
    if roots.is_empty() {
        report.push(ValidationIssue { node: None, edge: None, error: ValidationError::MissingRoot });
    }

    let mut reachable: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut queue: Vec<&str> = roots.clone();
    while let Some(cur) = queue.pop() {
        if !reachable.insert(cur) {
            continue;
        }
        for e in &ir.edges {
            if e.from == cur {
                queue.push(e.to.as_str());
            }
        }
    }
    for n in &ir.nodes {
        if !reachable.contains(n.id.as_str()) {
            report.push(ValidationIssue {
                node: Some(n.id.clone()),
                edge: None,
                error: ValidationError::UnreachableNode(n.id.clone()),
            });
        }
    }
}

pub(crate) fn run_all(ir: &WorkflowIR, report: &mut ValidationReport) {
    structural_checks(ir, report);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ir_with(nodes: Vec<WorkflowNode>, edges: Vec<WorkflowEdge>) -> WorkflowIR {
        WorkflowIR {
            version: WORKFLOW_IR_VERSION,
            workflow_id: Uuid::new_v4(),
            nodes,
            edges,
            metadata: WorkflowMetadata::default(),
        }
    }

    fn node(id: &str, kind: WorkflowNodeKind) -> WorkflowNode {
        WorkflowNode {
            id: id.into(),
            kind,
            capability: None,
            config: BTreeMap::new(),
        }
    }

    fn edge(from: &str, to: &str, kind: WorkflowEdgeKind) -> WorkflowEdge {
        WorkflowEdge {
            from: from.into(),
            to: to.into(),
            kind,
            condition: None,
        }
    }

    #[test]
    fn reports_duplicate_node_ids() {
        let ir = ir_with(vec![node("n1", WorkflowNodeKind::Task), node("n1", WorkflowNodeKind::Output)], vec![]);
        let report = ir.validate();
        assert!(report.issues().iter().any(|i| i.error == ValidationError::DuplicateNodeId("n1".into())));
    }

    #[test]
    fn reports_dangling_edge_reference() {
        let ir = ir_with(vec![node("n1", WorkflowNodeKind::Task)], vec![edge("n1", "ghost", WorkflowEdgeKind::Sequential)]);
        let report = ir.validate();
        assert!(report.issues().iter().any(|i| i.error == ValidationError::UnknownNodeRef("ghost".into())));
    }

    #[test]
    fn reports_missing_root_and_unreachable_nodes() {
        let cycle = ir_with(
            vec![node("a", WorkflowNodeKind::Task), node("b", WorkflowNodeKind::Task)],
            vec![edge("a", "b", WorkflowEdgeKind::Sequential), edge("b", "a", WorkflowEdgeKind::Loop)],
        );
        let report = cycle.validate();
        assert!(report.issues().iter().any(|i| i.error == ValidationError::MissingRoot));
        assert!(!report.is_empty());
        assert!(report.first_error().is_some());
    }

    #[test]
    fn valid_graph_passes_structural_checks() {
        let ir = ir_with(
            vec![node("a", WorkflowNodeKind::Task), node("b", WorkflowNodeKind::Output)],
            vec![edge("a", "b", WorkflowEdgeKind::Sequential)],
        );
        assert!(ir.validate().is_empty());
    }

    #[test]
    fn report_order_is_deterministic() {
        let ir = ir_with(
            vec![node("n1", WorkflowNodeKind::Task), node("n1", WorkflowNodeKind::Task)],
            vec![edge("n1", "ghost", WorkflowEdgeKind::Sequential)],
        );
        let first = ir.validate();
        let second = ir.validate();
        assert_eq!(first.issues(), second.issues());
    }
}
