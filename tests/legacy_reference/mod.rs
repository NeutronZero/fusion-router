//! Frozen Pre-Convergence Legacy Monolith Reference Passes
//!
//! Extracted from git history (`commit 9dcd55a4f7c4d9b46f89e0381afabdc0043d1c66~1`)
//! purely for side-by-side equivalence testing against `crates/fusion-compiler`.
//!
//! This code is frozen and read-only. It exists solely to prove that porting to `crates/`
//! preserved 100% of historical compiler pass behavior.

use async_trait::async_trait;
use fusion_types::{CompilerError, IRNodeKind, ModelCatalog, WorkflowIR};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

fn val_err(pass: &str, node_id: Option<Uuid>, msg: String) -> CompilerError {
    CompilerError::ValidationError {
        pass: pass.to_string(),
        node_id,
        message: msg,
    }
}

#[allow(dead_code)]
#[async_trait]
pub trait LegacyCompilerPass: Send + Sync {
    fn name(&self) -> &str;
    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError>;
}

// ---------------------------------------------------------------------------
// 1. Legacy ConstraintValidationPass
// ---------------------------------------------------------------------------

pub struct LegacyConstraintValidationPass;

#[async_trait]
impl LegacyCompilerPass for LegacyConstraintValidationPass {
    fn name(&self) -> &str {
        "constraint_validation"
    }

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        if ir.nodes.is_empty() {
            return Err(val_err(
                "constraint_validation",
                None,
                "IR must have at least one node".into(),
            ));
        }
        Ok(ir)
    }
}

// ---------------------------------------------------------------------------
// 2. Legacy ModelResolutionPass
// ---------------------------------------------------------------------------

pub struct LegacyModelResolutionPass {
    pub model_catalog: ModelCatalog,
}

impl LegacyModelResolutionPass {
    pub fn new(model_catalog: ModelCatalog) -> Self {
        Self { model_catalog }
    }

    pub fn select_model(&self) -> &str {
        &self.model_catalog.fast
    }
}

#[async_trait]
impl LegacyCompilerPass for LegacyModelResolutionPass {
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
                | IRNodeKind::Barrier => {}
                _ => {
                    if node.model.is_none() {
                        node.model = Some(self.select_model().to_string());
                    }
                }
            }
        }
        Ok(ir)
    }
}

// ---------------------------------------------------------------------------
// 3. Legacy ControlFlowValidationPass
// ---------------------------------------------------------------------------

pub struct LegacyControlFlowValidationPass;

#[async_trait]
impl LegacyCompilerPass for LegacyControlFlowValidationPass {
    fn name(&self) -> &str {
        "control_flow_validation"
    }

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        let node_ids: HashSet<Uuid> = ir.nodes.iter().map(|n| n.id).collect();

        // Validate edge references
        for edge in &ir.edges {
            if !node_ids.contains(&edge.from) {
                return Err(val_err(
                    "control_flow_validation",
                    None,
                    format!("Edge from {} references unknown source node", edge.from),
                ));
            }
            if !node_ids.contains(&edge.to) {
                return Err(val_err(
                    "control_flow_validation",
                    None,
                    format!("Edge to {} references unknown target node", edge.to),
                ));
            }
        }

        // Validate per-kind invariants
        for node in &ir.nodes {
            match node.kind {
                IRNodeKind::Conditional => {
                    let outgoing: Vec<&fusion_types::IREdge> =
                        ir.edges.iter().filter(|e| e.from == node.id).collect();
                    if outgoing.is_empty() {
                        return Err(val_err(
                            "control_flow_validation",
                            Some(node.id),
                            "Conditional node must have at least one outgoing edge".to_string(),
                        ));
                    }
                    if !outgoing.iter().any(|e| e.condition.is_some()) {
                        return Err(val_err(
                            "control_flow_validation",
                            Some(node.id),
                            "Conditional node must have at least one edge with a condition"
                                .to_string(),
                        ));
                    }
                }
                IRNodeKind::Loop => {
                    let outgoing: Vec<&fusion_types::IREdge> =
                        ir.edges.iter().filter(|e| e.from == node.id).collect();
                    if outgoing.is_empty() {
                        return Err(val_err(
                            "control_flow_validation",
                            Some(node.id),
                            "Loop node must have at least one outgoing edge".to_string(),
                        ));
                    }
                    if !node.config.contains_key("max_iterations") {
                        return Err(val_err(
                            "control_flow_validation",
                            Some(node.id),
                            "Loop node must have max_iterations in config".to_string(),
                        ));
                    }
                }
                IRNodeKind::Split => {
                    let outgoing: Vec<&fusion_types::IREdge> =
                        ir.edges.iter().filter(|e| e.from == node.id).collect();
                    if outgoing.len() < 2 {
                        return Err(val_err(
                            "control_flow_validation",
                            Some(node.id),
                            format!(
                                "Split node must have at least 2 outgoing edges, got {}",
                                outgoing.len()
                            ),
                        ));
                    }
                }
                IRNodeKind::Join => {
                    let incoming: Vec<&fusion_types::IREdge> =
                        ir.edges.iter().filter(|e| e.to == node.id).collect();
                    if incoming.len() < 2 {
                        return Err(val_err(
                            "control_flow_validation",
                            Some(node.id),
                            format!(
                                "Join node must have at least 2 incoming edges, got {}",
                                incoming.len()
                            ),
                        ));
                    }
                }
                IRNodeKind::Barrier => {
                    let outgoing: Vec<&fusion_types::IREdge> =
                        ir.edges.iter().filter(|e| e.from == node.id).collect();
                    let incoming: Vec<&fusion_types::IREdge> =
                        ir.edges.iter().filter(|e| e.to == node.id).collect();
                    if incoming.is_empty() {
                        return Err(val_err(
                            "control_flow_validation",
                            Some(node.id),
                            "Barrier node must have at least one incoming edge".to_string(),
                        ));
                    }
                    if outgoing.is_empty() {
                        return Err(val_err(
                            "control_flow_validation",
                            Some(node.id),
                            "Barrier node must have at least one outgoing edge".to_string(),
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

impl LegacyControlFlowValidationPass {
    fn detect_illegal_cycles(&self, ir: &WorkflowIR) -> Result<(), CompilerError> {
        let edges: Vec<(Uuid, Uuid)> = ir
            .edges
            .iter()
            .filter(|e| e.condition.as_deref() != Some("loop"))
            .map(|e| (e.from, e.to))
            .collect();

        match three_color_cycle_detect(&edges) {
            Ok(()) => Ok(()),
            Err(node_id) => Err(val_err(
                "control_flow_validation",
                Some(node_id),
                "Illegal cycle detected outside of loop back-edges".into(),
            )),
        }
    }
}

fn three_color_cycle_detect(edges: &[(Uuid, Uuid)]) -> Result<(), Uuid> {
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Grey,
        Black,
    }

    let mut colors: HashMap<Uuid, Color> = HashMap::new();
    let mut graph: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (from, to) in edges {
        graph.entry(*from).or_default().push(*to);
        graph.entry(*to).or_default();
    }

    fn dfs(
        node: Uuid,
        graph: &HashMap<Uuid, Vec<Uuid>>,
        colors: &mut HashMap<Uuid, Color>,
    ) -> bool {
        colors.insert(node, Color::Grey);
        if let Some(neighbors) = graph.get(&node) {
            for &next in neighbors {
                match colors.get(&next).unwrap_or(&Color::White) {
                    Color::Grey => return true,
                    Color::White => {
                        if dfs(next, graph, colors) {
                            return true;
                        }
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
            && dfs(node, &graph, &mut colors)
        {
            return Err(node);
        }
    }

    Ok(())
}
