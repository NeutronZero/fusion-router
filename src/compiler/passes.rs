use async_trait::async_trait;
use std::collections::HashSet;
use uuid::Uuid;
use crate::types::{CompilerError, IRNodeKind};
use super::CompilerPass;
use crate::types::WorkflowIR;

pub struct ConstraintValidationPass;

#[async_trait]
impl CompilerPass for ConstraintValidationPass {
    fn name(&self) -> &str {
        "constraint_validation"
    }

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        if ir.nodes.is_empty() {
            return Err(CompilerError::ValidationError("IR must have at least one node".to_string()));
        }
        Ok(ir)
    }
}

pub struct ModelResolutionPass;

#[async_trait]
impl CompilerPass for ModelResolutionPass {
    fn name(&self) -> &str {
        "model_resolution"
    }

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
                        node.model = Some("claude-sonnet-4-20250514".to_string());
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

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        let temp_graph = super::lower_to_graph(ir.clone())?;
        if !self.resource_manager.can_afford(&temp_graph).await {
            return Err(CompilerError::ValidationError("Budget exceeded".to_string()));
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

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        let node_ids: HashSet<Uuid> = ir.nodes.iter().map(|n| n.id).collect();

        for edge in &ir.edges {
            if !node_ids.contains(&edge.from) {
                return Err(CompilerError::ValidationError(
                    format!("Edge from {} references unknown source node", edge.from)
                ));
            }
            if !node_ids.contains(&edge.to) {
                return Err(CompilerError::ValidationError(
                    format!("Edge to {} references unknown target node", edge.to)
                ));
            }
        }

        for node in &ir.nodes {
            match node.kind {
                IRNodeKind::Conditional => {
                    let outgoing: Vec<&crate::types::IREdge> = ir.edges.iter()
                        .filter(|e| e.from == node.id)
                        .collect();
                    if outgoing.is_empty() {
                        return Err(CompilerError::ValidationError(
                            format!("Conditional node {} must have at least one outgoing edge", node.id)
                        ));
                    }
                    if !outgoing.iter().any(|e| e.condition.is_some()) {
                        return Err(CompilerError::ValidationError(
                            format!("Conditional node {} must have at least one edge with a condition", node.id)
                        ));
                    }
                }
                IRNodeKind::Loop => {
                    let outgoing: Vec<&crate::types::IREdge> = ir.edges.iter()
                        .filter(|e| e.from == node.id)
                        .collect();
                    if outgoing.is_empty() {
                        return Err(CompilerError::ValidationError(
                            format!("Loop node {} must have at least one outgoing edge", node.id)
                        ));
                    }
                    if !node.config.contains_key("max_iterations") {
                        return Err(CompilerError::ValidationError(
                            format!("Loop node {} must have max_iterations in config", node.id)
                        ));
                    }
                }
                IRNodeKind::Split => {
                    let outgoing: Vec<&crate::types::IREdge> = ir.edges.iter()
                        .filter(|e| e.from == node.id)
                        .collect();
                    if outgoing.len() < 2 {
                        return Err(CompilerError::ValidationError(
                            format!("Split node {} must have at least 2 outgoing edges, got {}", node.id, outgoing.len())
                        ));
                    }
                }
                IRNodeKind::Join => {
                    let incoming: Vec<&crate::types::IREdge> = ir.edges.iter()
                        .filter(|e| e.to == node.id)
                        .collect();
                    if incoming.len() < 2 {
                        return Err(CompilerError::ValidationError(
                            format!("Join node {} must have at least 2 incoming edges, got {}", node.id, incoming.len())
                        ));
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
                        return Err(CompilerError::ValidationError(
                            format!("Barrier node {} must have at least one incoming edge", node.id)
                        ));
                    }
                    if outgoing.is_empty() {
                        return Err(CompilerError::ValidationError(
                            format!("Barrier node {} must have at least one outgoing edge", node.id)
                        ));
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
    fn detect_illegal_cycles(&self, ir: &WorkflowIR) -> Result<(), CompilerError> {
        let mut visited: HashSet<Uuid> = HashSet::new();
        let mut stack: HashSet<Uuid> = HashSet::new();
        let empty_set = HashSet::new();

        for node in &ir.nodes {
            if !visited.contains(&node.id) {
                if self.has_cycle(node.id, ir, &mut visited, &mut stack, &empty_set) {
                    return Err(CompilerError::ValidationError(
                        "Illegal cycle detected outside of loop back-edges".to_string()
                    ));
                }
            }
        }

        Ok(())
    }

    fn has_cycle(
        &self,
        node_id: Uuid,
        ir: &WorkflowIR,
        visited: &mut HashSet<Uuid>,
        stack: &mut HashSet<Uuid>,
        _loop_back_ids: &HashSet<Uuid>,
    ) -> bool {
        if stack.contains(&node_id) {
            return true;
        }
        if visited.contains(&node_id) {
            return false;
        }

        visited.insert(node_id);
        stack.insert(node_id);

        let outgoing: Vec<Uuid> = ir.edges.iter()
            .filter(|e| {
                if e.from == node_id && e.condition.as_deref() == Some("loop") {
                    return false;
                }
                e.from == node_id
            })
            .map(|e| e.to)
            .collect();

        for next_id in outgoing {
            if self.has_cycle(next_id, ir, visited, stack, _loop_back_ids) {
                return true;
            }
        }

        stack.remove(&node_id);
        false
    }
}
