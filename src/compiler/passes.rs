use async_trait::async_trait;
use std::collections::HashSet;
use uuid::Uuid;
use crate::types::{CompilerError, ExecutionGraph, GraphMetadata, IRNodeKind, ModelCatalog};
use super::CompilerPass;
use crate::types::WorkflowIR;

fn val_err(pass: &str, node_id: Option<Uuid>, msg: String) -> CompilerError {
    CompilerError::ValidationError { pass: pass.to_string(), node_id, message: msg }
}

#[allow(dead_code)]
fn pass_err(pass: &str, msg: String) -> CompilerError {
    CompilerError::PassError { pass: pass.to_string(), message: msg }
}

pub struct ConstraintValidationPass;

#[async_trait]
impl CompilerPass for ConstraintValidationPass {
    fn name(&self) -> &str {
        "constraint_validation"
    }

    #[tracing::instrument(skip_all, fields(pass = self.name(), node_count = ir.nodes.len()))]
    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        if ir.nodes.is_empty() {
            return Err(val_err("constraint_validation", None, "IR must have at least one node".into()));
        }
        Ok(ir)
    }
}

pub struct ModelResolutionPass {
    pub model_catalog: ModelCatalog,
}

#[async_trait]
impl CompilerPass for ModelResolutionPass {
    fn name(&self) -> &str {
        "model_resolution"
    }

    #[tracing::instrument(skip_all, fields(pass = self.name(), node_count = ir.nodes.len()))]
    async fn apply(&self, mut ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        for node in &mut ir.nodes {
            match node.kind {
                IRNodeKind::Conditional
                | IRNodeKind::Loop
                | IRNodeKind::Split
                | IRNodeKind::Join
                | IRNodeKind::Barrier => {
                    // Control flow nodes don't need a model
                }
                _ => {
                    if node.model.is_none() {
                        node.model = Some(self.model_catalog.fast.clone());
                    }
                }
            }
        }
        Ok(ir)
    }
}

use std::sync::Arc;
use crate::resource::ResourceManager;

pub struct BudgetOptimisationPass {
    pub resource_manager: Arc<dyn ResourceManager>,
}

#[async_trait]
impl CompilerPass for BudgetOptimisationPass {
    fn name(&self) -> &str {
        "budget_optimisation"
    }

    #[tracing::instrument(skip_all, fields(pass = self.name(), node_count = ir.nodes.len()))]
    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        let budget_graph = ExecutionGraph {
            graph_id: ir.plan_id,
            nodes: vec![],
            edges: vec![],
            metadata: GraphMetadata {
                estimated_cost: ir.metadata.estimated_cost,
                estimated_tokens: ir.metadata.estimated_tokens,
                max_depth: 0,
                node_count: ir.nodes.len() as u32,
            },
            total_tokens: ir.metadata.estimated_tokens,
            total_cost: ir.metadata.estimated_cost.ceil() as u64,
        };
        if !self.resource_manager.can_afford(&budget_graph).await {
            return Err(val_err("budget_optimisation", None, "Budget exceeded".into()));
        }
        Ok(ir)
    }
}

pub struct ControlFlowValidationPass;

#[async_trait]
impl CompilerPass for ControlFlowValidationPass {
    fn name(&self) -> &str {
        "control_flow_validation"
    }

    #[tracing::instrument(skip_all, fields(pass = self.name(), node_count = ir.nodes.len(), edge_count = ir.edges.len()))]
    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        let node_ids: HashSet<Uuid> = ir.nodes.iter().map(|n| n.id).collect();

        for edge in &ir.edges {
            if !node_ids.contains(&edge.from) {
                return Err(val_err("control_flow_validation", None,
                    format!("Edge from {} references unknown source node", edge.from)));
            }
            if !node_ids.contains(&edge.to) {
                return Err(val_err("control_flow_validation", None,
                    format!("Edge to {} references unknown target node", edge.to)));
            }
        }

        for node in &ir.nodes {
            match node.kind {
                IRNodeKind::Conditional => {
                    let outgoing: Vec<&crate::types::IREdge> = ir.edges.iter()
                        .filter(|e| e.from == node.id)
                        .collect();
                    if outgoing.is_empty() {
                        return Err(val_err("control_flow_validation", Some(node.id),
                            "Conditional node must have at least one outgoing edge".to_string()));
                    }
                    if !outgoing.iter().any(|e| e.condition.is_some()) {
                        return Err(val_err("control_flow_validation", Some(node.id),
                            "Conditional node must have at least one edge with a condition".to_string()));
                    }
                }
                IRNodeKind::Loop => {
                    let outgoing: Vec<&crate::types::IREdge> = ir.edges.iter()
                        .filter(|e| e.from == node.id)
                        .collect();
                    if outgoing.is_empty() {
                        return Err(val_err("control_flow_validation", Some(node.id),
                            "Loop node must have at least one outgoing edge".to_string()));
                    }
                    if !node.config.contains_key("max_iterations") {
                        return Err(val_err("control_flow_validation", Some(node.id),
                            "Loop node must have max_iterations in config".to_string()));
                    }
                }
                IRNodeKind::Split => {
                    let outgoing: Vec<&crate::types::IREdge> = ir.edges.iter()
                        .filter(|e| e.from == node.id)
                        .collect();
                    if outgoing.len() < 2 {
                        return Err(val_err("control_flow_validation", Some(node.id),
                            format!("Split node must have at least 2 outgoing edges, got {}", outgoing.len())));
                    }
                }
                IRNodeKind::Join => {
                    let incoming: Vec<&crate::types::IREdge> = ir.edges.iter()
                        .filter(|e| e.to == node.id)
                        .collect();
                    if incoming.len() < 2 {
                        return Err(val_err("control_flow_validation", Some(node.id),
                            format!("Join node must have at least 2 incoming edges, got {}", incoming.len())));
                    }
                }
                IRNodeKind::Barrier => {
                    let outgoing: Vec<&crate::types::IREdge> = ir.edges.iter()
                        .filter(|e| e.from == node.id)
                        .collect();
                    let incoming: Vec<&crate::types::IREdge> = ir.edges.iter()
                        .filter(|e| e.to == node.id)
                        .collect();
                    if incoming.is_empty() {
                        return Err(val_err("control_flow_validation", Some(node.id),
                            "Barrier node must have at least one incoming edge".to_string()));
                    }
                    if outgoing.is_empty() {
                        return Err(val_err("control_flow_validation", Some(node.id),
                            "Barrier node must have at least one outgoing edge".to_string()));
                    }
                }
                _ => {}
            }
        }

        self.detect_illegal_cycles(&ir)?;

        Ok(ir)
    }
}

impl ControlFlowValidationPass {
    #[tracing::instrument(skip(self, ir))]
    fn detect_illegal_cycles(&self, ir: &WorkflowIR) -> Result<(), CompilerError> {
        let edges: Vec<(Uuid, Uuid)> = ir.edges.iter()
            .filter(|e| e.condition.as_deref() != Some("loop"))
            .map(|e| (e.from, e.to))
            .collect();

        match three_color_cycle_detect(&edges) {
            Ok(()) => Ok(()),
            Err(node_id) => Err(val_err("control_flow_validation", Some(node_id),
                "Illegal cycle detected outside of loop back-edges".into())),
        }
    }
}

fn three_color_cycle_detect(edges: &[(Uuid, Uuid)]) -> Result<(), Uuid> {
    #[derive(Clone, Copy, PartialEq)]
    enum Color { White, Grey, Black }

    let mut colors: std::collections::HashMap<Uuid, Color> = std::collections::HashMap::new();
    let mut graph: std::collections::HashMap<Uuid, Vec<Uuid>> = std::collections::HashMap::new();
    for (from, to) in edges {
        graph.entry(*from).or_default().push(*to);
        graph.entry(*to).or_default();
    }

    fn dfs(
        node: Uuid,
        graph: &std::collections::HashMap<Uuid, Vec<Uuid>>,
        colors: &mut std::collections::HashMap<Uuid, Color>,
    ) -> bool {
        colors.insert(node, Color::Grey);
        if let Some(neighbors) = graph.get(&node) {
            for &next in neighbors {
                match colors.get(&next).unwrap_or(&Color::White) {
                    Color::Grey => return true,
                    Color::White => {
                        if dfs(next, graph, colors) { return true; }
                    }
                    Color::Black => continue,
                }
            }
        }
        colors.insert(node, Color::Black);
        false
    }

    for node in graph.keys().copied().collect::<Vec<_>>() {
        if colors.get(&node).unwrap_or(&Color::White) == &Color::White
            && dfs(node, &graph, &mut colors) {
                return Err(node);
            }
    }

    Ok(())
}
