#[cfg(test)]
use crate::edge::WorkflowEdge;
use crate::edge::WorkflowEdgeKind;
#[cfg(test)]
use crate::node::WorkflowNode;
use crate::node::WorkflowNodeKind;
use crate::version::WORKFLOW_IR_VERSION;
use crate::workflow::WorkflowIR;
#[cfg(test)]
use crate::workflow::WorkflowMetadata;
#[cfg(test)]
use std::collections::BTreeMap;
use thiserror::Error;
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
    #[error("split node `{0}` requires at least two outgoing edges")]
    SplitArity(String),
    #[error("loop edge source `{0}` requires `max_iterations` in config")]
    LoopConfig(String),
    #[error("barrier node `{0}` requires at least one incoming and one outgoing edge")]
    BarrierArity(String),
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

    // Root detection skips Loop edges (an iteration back-edge into a loop
    // head must not make the head its own root), but reachability BELOW
    // walks every edge kind including Loop, so nodes entered only through a
    // loop edge are not spuriously flagged unreachable (review M7).
    let mut incoming: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut adjacency: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for e in &ir.edges {
        if e.kind != WorkflowEdgeKind::Loop && seen.contains(e.to.as_str()) {
            *incoming.entry(e.to.as_str()).or_insert(0) += 1;
        }
        adjacency
            .entry(e.from.as_str())
            .or_default()
            .push(e.to.as_str());
    }
    let roots: Vec<&str> = ir
        .nodes
        .iter()
        .map(|n| n.id.as_str())
        .filter(|id| incoming.get(id).copied().unwrap_or(0) == 0)
        .collect();
    if roots.is_empty() {
        report.push(ValidationIssue {
            node: None,
            edge: None,
            error: ValidationError::MissingRoot,
        });
    }

    let mut reachable: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut queue: Vec<&str> = roots.clone();
    while let Some(cur) = queue.pop() {
        if !reachable.insert(cur) {
            continue;
        }
        if let Some(nexts) = adjacency.get(cur) {
            for next in nexts {
                queue.push(next);
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

fn control_flow_marker<'a>(ir: &'a WorkflowIR, id: &str) -> Option<&'a str> {
    ir.nodes
        .iter()
        .find(|n| n.id == id)
        .and_then(|n| n.config.get("control_flow").and_then(|v| v.as_str()))
}

/// Number of DISTINCT source nodes feeding `id` via Merge edges. Duplicate
/// parallel edges between the same pair must not satisfy merge arity on
/// their own (review M7).
fn merge_sources(ir: &WorkflowIR, id: &str) -> usize {
    let mut sources: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for e in &ir.edges {
        if e.kind == WorkflowEdgeKind::Merge && e.to == id {
            sources.insert(e.from.as_str());
        }
    }
    sources.len()
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
                        error: ValidationError::MissingCondition {
                            from: e.from.clone(),
                            to: e.to.clone(),
                        },
                    });
                }
            }
            WorkflowEdgeKind::Retry => {
                if !matches!(
                    node_kind_of(ir, &e.from),
                    Some(
                        WorkflowNodeKind::Task
                            | WorkflowNodeKind::Tool
                            | WorkflowNodeKind::Retrieval
                    )
                ) {
                    report.push(ValidationIssue {
                        node: None,
                        edge: edge_ref,
                        error: ValidationError::NotRetryable(e.from.clone()),
                    });
                }
            }
            WorkflowEdgeKind::Merge
                if merge_sources(ir, &e.to) < 2 =>
            {
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
        if n.kind == WorkflowNodeKind::Output
            && outgoing.get(n.id.as_str()).copied().unwrap_or(0) > 0
        {
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

    for e in &ir.edges {
        if e.kind == WorkflowEdgeKind::Loop {
            if let Some(marker) = control_flow_marker(ir, &e.from) {
                if marker == "loop" {
                    let has_max = ir
                        .nodes
                        .iter()
                        .find(|n| n.id == e.from)
                        .map(|n| n.config.contains_key("max_iterations"))
                        .unwrap_or(false);
                    if !has_max {
                        report.push(ValidationIssue {
                            node: Some(e.from.clone()),
                            edge: Some(format!("{} -> {}", e.from, e.to)),
                            error: ValidationError::LoopConfig(e.from.clone()),
                        });
                    }
                }
            }
        }
    }

    for n in &ir.nodes {
        if let Some(marker) = control_flow_marker(ir, &n.id) {
            if marker == "split" {
                let out = outgoing.get(n.id.as_str()).copied().unwrap_or(0);
                if out < 2 {
                    report.push(ValidationIssue {
                        node: Some(n.id.clone()),
                        edge: None,
                        error: ValidationError::SplitArity(n.id.clone()),
                    });
                }
            } else if marker == "barrier" {
                let inc = incoming.get(n.id.as_str()).copied().unwrap_or(0);
                let out = outgoing.get(n.id.as_str()).copied().unwrap_or(0);
                if inc < 1 || out < 1 {
                    report.push(ValidationIssue {
                        node: Some(n.id.clone()),
                        edge: None,
                        error: ValidationError::BarrierArity(n.id.clone()),
                    });
                }
            }
        }
    }
}

fn non_loop_cycle_back_edges(ir: &WorkflowIR) -> Vec<(String, String)> {
    let mut adjacency: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for e in &ir.edges {
        if e.kind != WorkflowEdgeKind::Loop {
            adjacency
                .entry(e.from.as_str())
                .or_default()
                .push(e.to.as_str());
        }
    }
    // Iterative 3-color DFS with an explicit stack: client-supplied IRs can
    // be arbitrarily deep, and a recursive walk would overflow the thread
    // stack long before allocation limits do (review H5).
    let mut back_edges = Vec::new();
    let mut state: std::collections::HashMap<&str, u8> = std::collections::HashMap::new(); // 0=unseen, 1=in-stack, 2=done
    for n in &ir.nodes {
        if state.get(n.id.as_str()).copied().unwrap_or(0) != 0 {
            continue;
        }
        // Stack entries: (node, next child index to visit).
        let mut stack: Vec<(&str, usize)> = vec![(n.id.as_str(), 0)];
        state.insert(n.id.as_str(), 1);
        while let Some((node, child_idx)) = stack.pop() {
            let children = adjacency.get(node).map(Vec::as_slice).unwrap_or(&[]);
            if child_idx < children.len() {
                let next = children[child_idx];
                // Re-push this node to resume at the following sibling.
                stack.push((node, child_idx + 1));
                match state.get(next).copied().unwrap_or(0) {
                    0 => {
                        state.insert(next, 1);
                        stack.push((next, 0));
                    }
                    1 => back_edges.push((node.to_string(), next.to_string())),
                    _ => {}
                }
            } else {
                state.insert(node, 2);
            }
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
            selected_model: None,
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
        let ir = ir_with(
            vec![
                node("n1", WorkflowNodeKind::Task),
                node("n1", WorkflowNodeKind::Output),
            ],
            vec![],
        );
        let report = ir.validate();
        assert!(report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::DuplicateNodeId("n1".into())));
    }

    #[test]
    fn reports_dangling_edge_reference() {
        let ir = ir_with(
            vec![node("n1", WorkflowNodeKind::Task)],
            vec![edge("n1", "ghost", WorkflowEdgeKind::Sequential)],
        );
        let report = ir.validate();
        assert!(report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::UnknownNodeRef("ghost".into())));
    }

    #[test]
    fn reports_missing_root_and_unreachable_nodes() {
        let cycle = ir_with(
            vec![
                node("a", WorkflowNodeKind::Task),
                node("b", WorkflowNodeKind::Task),
            ],
            vec![
                edge("a", "b", WorkflowEdgeKind::Sequential),
                edge("b", "a", WorkflowEdgeKind::Sequential),
            ],
        );
        let report = cycle.validate();
        assert!(report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::MissingRoot));
        assert!(!report.is_empty());
        assert!(report.first_error().is_some());
    }

    #[test]
    fn valid_graph_passes_structural_checks() {
        let ir = ir_with(
            vec![
                node("a", WorkflowNodeKind::Task),
                node("b", WorkflowNodeKind::Output),
            ],
            vec![edge("a", "b", WorkflowEdgeKind::Sequential)],
        );
        assert!(ir.validate().is_empty());
    }

    #[test]
    fn report_order_is_deterministic() {
        let ir = ir_with(
            vec![
                node("n1", WorkflowNodeKind::Task),
                node("n1", WorkflowNodeKind::Task),
            ],
            vec![edge("n1", "ghost", WorkflowEdgeKind::Sequential)],
        );
        let first = ir.validate();
        let second = ir.validate();
        assert_eq!(first.issues(), second.issues());
    }

    #[test]
    fn only_loop_edges_may_cycle() {
        let ir = ir_with(
            vec![
                node("a", WorkflowNodeKind::Task),
                node("b", WorkflowNodeKind::Task),
                node("c", WorkflowNodeKind::Output),
            ],
            vec![
                edge("a", "b", WorkflowEdgeKind::Sequential),
                edge("b", "a", WorkflowEdgeKind::Sequential),
                edge("a", "c", WorkflowEdgeKind::Sequential),
            ],
        );
        let report = ir.validate();
        assert!(report.issues().iter().any(|i| i.error
            == ValidationError::IllegalCycle {
                from: "b".into(),
                to: "a".into()
            }));
    }

    #[test]
    fn loop_edge_cycle_is_legal() {
        let ir = ir_with(
            vec![
                node("a", WorkflowNodeKind::Task),
                node("b", WorkflowNodeKind::Output),
            ],
            vec![
                edge("a", "b", WorkflowEdgeKind::Sequential),
                edge("b", "a", WorkflowEdgeKind::Loop),
            ],
        );
        let report = ir.validate();
        assert!(!report
            .issues()
            .iter()
            .any(|i| matches!(i.error, ValidationError::IllegalCycle { .. })));
    }

    #[test]
    fn conditional_edge_requires_condition() {
        let ir = ir_with(
            vec![
                node("a", WorkflowNodeKind::Task),
                node("b", WorkflowNodeKind::Output),
            ],
            vec![WorkflowEdge {
                from: "a".into(),
                to: "b".into(),
                kind: WorkflowEdgeKind::Conditional,
                condition: None,
            }],
        );
        let report = ir.validate();
        assert!(report.issues().iter().any(|i| i.error
            == ValidationError::MissingCondition {
                from: "a".into(),
                to: "b".into()
            }));
    }

    #[test]
    fn retry_edge_requires_retryable_source() {
        let ir = ir_with(
            vec![
                node("a", WorkflowNodeKind::Output),
                node("b", WorkflowNodeKind::Task),
            ],
            vec![edge("a", "b", WorkflowEdgeKind::Retry)],
        );
        let report = ir.validate();
        assert!(report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::NotRetryable("a".into())));
    }

    #[test]
    fn merge_requires_two_incoming_edges() {
        let ir = ir_with(
            vec![
                node("a", WorkflowNodeKind::Task),
                node("b", WorkflowNodeKind::Task),
                node("m", WorkflowNodeKind::Aggregation),
                node("o", WorkflowNodeKind::Output),
            ],
            vec![
                edge("a", "m", WorkflowEdgeKind::Merge),
                edge("m", "o", WorkflowEdgeKind::Sequential),
            ],
        );
        let report = ir.validate();
        assert!(report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::MergeArity("m".into())));
    }

    #[test]
    fn output_node_may_not_have_outgoing_edges() {
        let ir = ir_with(
            vec![
                node("a", WorkflowNodeKind::Output),
                node("b", WorkflowNodeKind::Task),
            ],
            vec![edge("a", "b", WorkflowEdgeKind::Sequential)],
        );
        let report = ir.validate();
        assert!(report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::OutputOutgoing("a".into())));
    }

    #[test]
    fn provider_fields_are_rejected() {
        let ir = ir_with(
            vec![WorkflowNode {
                id: "a".into(),
                kind: WorkflowNodeKind::Task,
                capability: None,
                selected_model: None,
                config: BTreeMap::from([(
                    "model".into(),
                    serde_json::Value::String("gpt-4".into()),
                )]),
            }],
            vec![edge("a", "a", WorkflowEdgeKind::Loop)],
        );
        let report = ir.validate();
        assert!(report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::ProviderField("model".into())));
    }

    #[test]
    fn version_mismatch_is_rejected() {
        let mut ir = ir_with(
            vec![node("a", WorkflowNodeKind::Task)],
            vec![edge("a", "a", WorkflowEdgeKind::Loop)],
        );
        ir.version = 99;
        let report = ir.validate();
        assert!(report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::VersionMismatch(99, WORKFLOW_IR_VERSION)));
    }

    #[test]
    fn split_requires_two_parallel_outgoing_edges() {
        let mut n1 = node("n1", WorkflowNodeKind::Task);
        n1.config.insert(
            "control_flow".into(),
            serde_json::Value::String("split".into()),
        );
        let ir = ir_with(
            vec![n1, node("n2", WorkflowNodeKind::Task)],
            vec![edge("n1", "n2", WorkflowEdgeKind::Sequential)],
        );
        let report = ir.validate();
        assert!(report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::SplitArity("n1".into())));
    }

    #[test]
    fn split_with_two_parallel_edges_passes() {
        let mut n1 = node("n1", WorkflowNodeKind::Task);
        n1.config.insert(
            "control_flow".into(),
            serde_json::Value::String("split".into()),
        );
        let ir = ir_with(
            vec![
                n1,
                node("n2", WorkflowNodeKind::Task),
                node("n3", WorkflowNodeKind::Task),
            ],
            vec![
                edge("n1", "n2", WorkflowEdgeKind::Parallel),
                edge("n1", "n3", WorkflowEdgeKind::Parallel),
            ],
        );
        let report = ir.validate();
        assert!(!report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::SplitArity("n1".into())));
    }

    #[test]
    fn marked_split_accepts_two_non_parallel_outgoing_edges() {
        let mut n1 = node("n1", WorkflowNodeKind::Task);
        n1.config.insert(
            "control_flow".into(),
            serde_json::Value::String("split".into()),
        );
        let ir = ir_with(
            vec![
                n1,
                node("n2", WorkflowNodeKind::Task),
                node("n3", WorkflowNodeKind::Task),
            ],
            vec![
                edge("n1", "n2", WorkflowEdgeKind::Sequential),
                edge("n1", "n3", WorkflowEdgeKind::Conditional),
            ],
        );
        let report = ir.validate();
        assert!(!report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::SplitArity("n1".into())));
    }

    #[test]
    fn loop_edge_requires_max_iterations_in_config() {
        let mut n1 = node("n1", WorkflowNodeKind::Task);
        n1.config.insert(
            "control_flow".into(),
            serde_json::Value::String("loop".into()),
        );
        let ir = ir_with(
            vec![n1, node("n2", WorkflowNodeKind::Task)],
            vec![edge("n1", "n2", WorkflowEdgeKind::Loop)],
        );
        let report = ir.validate();
        assert!(report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::LoopConfig("n1".into())));
    }

    #[test]
    fn loop_edge_with_max_iterations_passes() {
        let mut n1 = node("n1", WorkflowNodeKind::Task);
        n1.config.insert(
            "control_flow".into(),
            serde_json::Value::String("loop".into()),
        );
        n1.config.insert(
            "max_iterations".into(),
            serde_json::Value::Number(10.into()),
        );
        let ir = ir_with(
            vec![n1, node("n2", WorkflowNodeKind::Task)],
            vec![edge("n1", "n2", WorkflowEdgeKind::Loop)],
        );
        let report = ir.validate();
        assert!(!report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::LoopConfig("n1".into())));
    }

    #[test]
    fn barrier_requires_incoming_and_outgoing_edges() {
        let mut n1 = node("n1", WorkflowNodeKind::Task);
        n1.config.insert(
            "control_flow".into(),
            serde_json::Value::String("barrier".into()),
        );
        let ir = ir_with(
            vec![n1, node("n2", WorkflowNodeKind::Task)],
            vec![edge("n2", "n1", WorkflowEdgeKind::Sequential)],
        );
        let report = ir.validate();
        assert!(report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::BarrierArity("n1".into())));
    }

    #[test]
    fn barrier_with_both_incoming_and_outgoing_passes() {
        let mut n1 = node("n1", WorkflowNodeKind::Task);
        n1.config.insert(
            "control_flow".into(),
            serde_json::Value::String("barrier".into()),
        );
        let ir = ir_with(
            vec![
                n1,
                node("n2", WorkflowNodeKind::Task),
                node("n3", WorkflowNodeKind::Task),
            ],
            vec![
                edge("n2", "n1", WorkflowEdgeKind::Sequential),
                edge("n1", "n3", WorkflowEdgeKind::Sequential),
            ],
        );
        let report = ir.validate();
        assert!(!report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::BarrierArity("n1".into())));
    }

    #[test]
    fn node_entered_only_via_loop_edge_is_reachable() {
        // Review M7: reachability must walk Loop edges even though root
        // detection skips them.
        let ir = ir_with(
            vec![
                node("a", WorkflowNodeKind::Task),
                node("b", WorkflowNodeKind::Task),
                node("c", WorkflowNodeKind::Output),
            ],
            vec![
                edge("a", "b", WorkflowEdgeKind::Sequential),
                edge("a", "c", WorkflowEdgeKind::Sequential),
                edge("b", "b", WorkflowEdgeKind::Loop),
            ],
        );
        let report = ir.validate();
        assert!(
            !report
                .issues()
                .iter()
                .any(|i| matches!(i.error, ValidationError::UnreachableNode(_))),
            "loop-entered nodes must count as reachable: {:?}",
            report.issues()
        );
    }

    #[test]
    fn duplicate_merge_edges_do_not_satisfy_arity() {
        let mut n1 = node("n1", WorkflowNodeKind::Task);
        n1.config.insert(
            "control_flow".into(),
            serde_json::Value::String("split".into()),
        );
        let m = node("m", WorkflowNodeKind::Aggregation);
        let ir = ir_with(
            vec![n1, m],
            vec![
                edge("n1", "m", WorkflowEdgeKind::Merge),
                edge("n1", "m", WorkflowEdgeKind::Merge),
            ],
        );
        let report = ir.validate();
        assert!(report
            .issues()
            .iter()
            .any(|i| i.error == ValidationError::MergeArity("m".into())));
    }

    #[test]
    fn deep_chain_does_not_overflow_stack() {
        // Review H5: cycle detection must be iterative; 50k chained nodes
        // previously recursed once per level.
        let count = 50_000usize;
        let nodes: Vec<WorkflowNode> = (0..count)
            .map(|i| node(&format!("n{i}"), WorkflowNodeKind::Task))
            .chain(std::iter::once(node(
                "out",
                WorkflowNodeKind::Output,
            )))
            .collect();
        let edges: Vec<WorkflowEdge> = (0..count)
            .map(|i| {
                if i + 1 < count {
                    edge(&format!("n{i}"), &format!("n{}", i + 1), WorkflowEdgeKind::Sequential)
                } else {
                    edge(&format!("n{i}"), "out", WorkflowEdgeKind::Sequential)
                }
            })
            .collect();
        let ir = ir_with(nodes, edges);
        assert!(
            ir.validate().is_empty(),
            "deep acyclic chain must validate cleanly"
        );
    }

    #[test]
    fn join_target_does_not_require_barrier_outgoing_edge() {
        let ir = ir_with(
            vec![
                node("a", WorkflowNodeKind::Task),
                node("b", WorkflowNodeKind::Task),
                node("m", WorkflowNodeKind::Aggregation),
            ],
            vec![
                edge("a", "m", WorkflowEdgeKind::Merge),
                edge("b", "m", WorkflowEdgeKind::Merge),
            ],
        );
        let report = ir.validate();
        assert!(!report
            .issues()
            .iter()
            .any(|i| matches!(i.error, ValidationError::BarrierArity(_))));
    }
}
