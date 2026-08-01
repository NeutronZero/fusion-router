use crate::edge::WorkflowEdgeKind;
use crate::node::WorkflowNodeKind;
use crate::version::WORKFLOW_IR_VERSION;
use crate::workflow::WorkflowIR;
use thiserror::Error;
#[cfg(test)]
use crate::edge::WorkflowEdge;
#[cfg(test)]
use crate::node::WorkflowNode;
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

const RESERVED_PROVIDER_KEYS: [&str; 3] = ["model", "provider", "endpoint"];

fn node_kind_of(ir: &WorkflowIR, id: &str) -> Option<WorkflowNodeKind> {
    ir.nodes.iter().find(|n| n.id == id).map(|n| n.kind)
}

fn semantic_checks(ir: &WorkflowIR, report: &mut ValidationReport) {
    let mut incoming: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut outgoing: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for e in &ir.edges {
        *incoming.entry(e.to.as_str()).or_insert(0) += 1;
        *outgoing.entry(e.from.as_str()).or_insert(0) += 1;
    }

    for e in &ir.edges {
        let edge_ref = Some(format!("{} -> {}", e.from, e.to));
        match e.kind {
            WorkflowEdgeKind::Conditional => {
                if e.condition.as_deref().unwrap_or("").is_empty() {
                    report.push(ValidationIssue {
                        node: None,
                        edge: edge_ref,
                        error: ValidationError::MissingCondition { from: e.from.clone(), to: e.to.clone() },
                    });
                }
            }
            WorkflowEdgeKind::Retry => {
                if !matches!(node_kind_of(ir, &e.from), Some(WorkflowNodeKind::Task | WorkflowNodeKind::Tool | WorkflowNodeKind::Retrieval)) {
                    report.push(ValidationIssue {
                        node: None,
                        edge: edge_ref,
                        error: ValidationError::NotRetryable(e.from.clone()),
                    });
                }
            }
            WorkflowEdgeKind::Merge if incoming.get(e.to.as_str()).copied().unwrap_or(0) < 2 => {
                report.push(ValidationIssue {
                    node: None,
                    edge: edge_ref,
                    error: ValidationError::MergeArity(e.to.clone()),
                });
            }
            _ => {}
        }
    }

    for n in &ir.nodes {
        if n.kind == WorkflowNodeKind::Output && outgoing.get(n.id.as_str()).copied().unwrap_or(0) > 0 {
            report.push(ValidationIssue {
                node: Some(n.id.clone()),
                edge: None,
                error: ValidationError::OutputOutgoing(n.id.clone()),
            });
        }
    }

    for (from, to) in non_loop_cycle_back_edges(ir) {
        report.push(ValidationIssue {
            node: None,
            edge: Some(format!("{} -> {}", from, to)),
            error: ValidationError::IllegalCycle { from, to },
        });
    }
}

fn non_loop_cycle_back_edges(ir: &WorkflowIR) -> Vec<(String, String)> {
    let mut adjacency: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for e in &ir.edges {
        if e.kind != WorkflowEdgeKind::Loop {
            adjacency.entry(e.from.as_str()).or_default().push(e.to.as_str());
        }
    }
    let mut back_edges = Vec::new();
    let mut state: std::collections::HashMap<&str, u8> = std::collections::HashMap::new(); // 0=unseen, 1=in-stack, 2=done
    fn dfs<'a>(
        node: &'a str,
        adjacency: &std::collections::HashMap<&'a str, Vec<&'a str>>,
        state: &mut std::collections::HashMap<&'a str, u8>,
        back_edges: &mut Vec<(String, String)>,
    ) {
        state.insert(node, 1);
        if let Some(nexts) = adjacency.get(node) {
            for next in nexts {
                match state.get(next).copied().unwrap_or(0) {
                    0 => dfs(next, adjacency, state, back_edges),
                    1 => back_edges.push((node.to_string(), next.to_string())),
                    _ => {}
                }
            }
        }
        state.insert(node, 2);
    }
    for n in &ir.nodes {
        if state.get(n.id.as_str()).copied().unwrap_or(0) == 0 {
            dfs(&n.id, &adjacency, &mut state, &mut back_edges);
        }
    }
    back_edges
}

fn scan_for_provider_keys(value: &serde_json::Value, path: &str, report: &mut ValidationReport) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                if RESERVED_PROVIDER_KEYS.contains(&k.as_str()) {
                    report.push(ValidationIssue {
                        node: Some(path.to_string()),
                        edge: None,
                        error: ValidationError::ProviderField(k.clone()),
                    });
                }
                scan_for_provider_keys(v, path, report);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                scan_for_provider_keys(item, path, report);
            }
        }
        _ => {}
    }
}

fn architectural_checks(ir: &WorkflowIR, report: &mut ValidationReport) {
    if ir.version != WORKFLOW_IR_VERSION {
        report.push(ValidationIssue {
            node: None,
            edge: None,
            error: ValidationError::VersionMismatch(ir.version, WORKFLOW_IR_VERSION),
        });
    }
    for n in &ir.nodes {
        let value = serde_json::to_value(&n.config).unwrap_or(serde_json::Value::Null);
        scan_for_provider_keys(&value, &n.id, report);
    }
}

pub(crate) fn run_all(ir: &WorkflowIR, report: &mut ValidationReport) {
    structural_checks(ir, report);
    semantic_checks(ir, report);
    architectural_checks(ir, report);
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

    #[test]
    fn only_loop_edges_may_cycle() {
        let ir = ir_with(
            vec![node("a", WorkflowNodeKind::Task), node("b", WorkflowNodeKind::Task), node("c", WorkflowNodeKind::Output)],
            vec![edge("a", "b", WorkflowEdgeKind::Sequential), edge("b", "a", WorkflowEdgeKind::Sequential), edge("a", "c", WorkflowEdgeKind::Sequential)],
        );
        let report = ir.validate();
        assert!(report.issues().iter().any(|i| i.error == ValidationError::IllegalCycle { from: "b".into(), to: "a".into() }));
    }

    #[test]
    fn loop_edge_cycle_is_legal() {
        let ir = ir_with(
            vec![node("a", WorkflowNodeKind::Task), node("b", WorkflowNodeKind::Output)],
            vec![edge("a", "b", WorkflowEdgeKind::Sequential), edge("b", "a", WorkflowEdgeKind::Loop)],
        );
        let report = ir.validate();
        assert!(!report.issues().iter().any(|i| matches!(i.error, ValidationError::IllegalCycle { .. })));
    }

    #[test]
    fn conditional_edge_requires_condition() {
        let ir = ir_with(
            vec![node("a", WorkflowNodeKind::Task), node("b", WorkflowNodeKind::Output)],
            vec![WorkflowEdge { from: "a".into(), to: "b".into(), kind: WorkflowEdgeKind::Conditional, condition: None }],
        );
        let report = ir.validate();
        assert!(report.issues().iter().any(|i| i.error == ValidationError::MissingCondition { from: "a".into(), to: "b".into() }));
    }

    #[test]
    fn retry_edge_requires_retryable_source() {
        let ir = ir_with(
            vec![node("a", WorkflowNodeKind::Output), node("b", WorkflowNodeKind::Task)],
            vec![edge("a", "b", WorkflowEdgeKind::Retry)],
        );
        let report = ir.validate();
        assert!(report.issues().iter().any(|i| i.error == ValidationError::NotRetryable("a".into())));
    }

    #[test]
    fn merge_requires_two_incoming_edges() {
        let ir = ir_with(
            vec![node("a", WorkflowNodeKind::Task), node("b", WorkflowNodeKind::Task), node("m", WorkflowNodeKind::Aggregation), node("o", WorkflowNodeKind::Output)],
            vec![
                edge("a", "m", WorkflowEdgeKind::Merge),
                edge("m", "o", WorkflowEdgeKind::Sequential),
            ],
        );
        let report = ir.validate();
        assert!(report.issues().iter().any(|i| i.error == ValidationError::MergeArity("m".into())));
    }

    #[test]
    fn output_node_may_not_have_outgoing_edges() {
        let ir = ir_with(
            vec![node("a", WorkflowNodeKind::Output), node("b", WorkflowNodeKind::Task)],
            vec![edge("a", "b", WorkflowEdgeKind::Sequential)],
        );
        let report = ir.validate();
        assert!(report.issues().iter().any(|i| i.error == ValidationError::OutputOutgoing("a".into())));
    }

    #[test]
    fn provider_fields_are_rejected() {
        let ir = ir_with(
            vec![WorkflowNode {
                id: "a".into(),
                kind: WorkflowNodeKind::Task,
                capability: None,
                config: BTreeMap::from([("model".into(), serde_json::Value::String("gpt-4".into()))]),
            }],
            vec![edge("a", "a", WorkflowEdgeKind::Loop)],
        );
        let report = ir.validate();
        assert!(report.issues().iter().any(|i| i.error == ValidationError::ProviderField("model".into())));
    }

    #[test]
    fn version_mismatch_is_rejected() {
        let mut ir = ir_with(vec![node("a", WorkflowNodeKind::Task)], vec![edge("a", "a", WorkflowEdgeKind::Loop)]);
        ir.version = 99;
        let report = ir.validate();
        assert!(report.issues().iter().any(|i| i.error == ValidationError::VersionMismatch(99, WORKFLOW_IR_VERSION)));
    }
}
