use crate::workflow::WorkflowIR;
use crate::error::WorkflowIrError;

fn edge_sort_key(e: &crate::edge::WorkflowEdge) -> (String, String, String, String) {
    (
        e.from.clone(),
        e.to.clone(),
        format!("{:?}", e.kind),
        e.condition.clone().unwrap_or_default(),
    )
}

pub(crate) fn to_canonical_json(ir: &WorkflowIR) -> Result<String, WorkflowIrError> {
    let mut nodes = ir.nodes.clone();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    let mut edges = ir.edges.clone();
    edges.sort_by(|a, b| edge_sort_key(a).cmp(&edge_sort_key(b)));
    let canonical = WorkflowIR {
        version: ir.version,
        workflow_id: ir.workflow_id,
        nodes,
        edges,
        metadata: ir.metadata.clone(),
    };
    Ok(serde_json::to_string(&canonical)?)
}

pub(crate) fn from_json(s: &str) -> Result<WorkflowIR, WorkflowIrError> {
    let ir: WorkflowIR = serde_json::from_str(s)?;
    let report = ir.validate();
    if let Some(first) = report.first_error() {
        return Err(WorkflowIrError::Validation(first.clone()));
    }
    Ok(ir)
}
