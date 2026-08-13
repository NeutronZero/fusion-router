//! Compiler pass pipeline for WorkflowIR optimisation.
//!
//! Passes operate on `fusion_types::WorkflowIR` (execution-layer IR) and produce
//! `fusion_types::ExecutionGraph`. The planning-level IR (`fusion_ir::WorkflowIR`)
//! is converted via the adapter before entering this pipeline.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use fusion_types::*;
use fusion_core::PlatformError;
use fusion_kernel::resource::StubResourceManager;
use serde::{Deserialize, Serialize};

pub mod passes;

/// Weights for sub-scores in the route scoring formula.
const WEIGHT_CAPABILITY: f64 = 0.3;
const WEIGHT_BUDGET: f64 = 0.25;
const WEIGHT_LATENCY: f64 = 0.2;
const WEIGHT_HEALTH: f64 = 0.15;
const WEIGHT_POLICY: f64 = 0.1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainRouteScore {
    pub provider_name: String,
    pub capability_score: Option<f64>,
    pub budget_score: Option<f64>,
    pub latency_score: Option<f64>,
    pub health_score: Option<f64>,
    pub policy_score: Option<f64>,
    pub total_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderComparisonCandidate {
    pub provider_name: String,
    pub model_name: String,
    pub total_score: f64,
    pub status: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilerPassDiff {
    pub pass_number: usize,
    pub pass_name: String,
    pub input_nodes: usize,
    pub output_nodes: usize,
    pub transformation_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilerReport {
    pub intent: String,
    pub passes_executed: Vec<String>,
    pub pass_diffs: Vec<CompilerPassDiff>,
    pub graph_id: String,
    pub compilation_time_ms: u64,
    pub route_scores: Vec<ExplainRouteScore>,
    pub provider_comparison: Vec<ProviderComparisonCandidate>,
}

#[async_trait::async_trait]
pub trait CompilerPass: Send + Sync {
    fn name(&self) -> &str;
    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError>;
}

#[async_trait::async_trait]
pub trait Compiler: Send + Sync {
    async fn compile(&self, ir: WorkflowIR) -> Result<ExecutionGraph, CompilerError>;
}

// ---------------------------------------------------------------------------
// Real passes (ported from src/compiler/passes/legacy_passes.rs)
// ---------------------------------------------------------------------------

pub struct ConstraintValidationPass;

#[async_trait::async_trait]
impl CompilerPass for ConstraintValidationPass {
    fn name(&self) -> &str { "constraint_validation" }

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        if ir.nodes.is_empty() {
            return Err(CompilerError::ValidationError {
                pass: "constraint_validation".into(),
                node_id: None,
                message: "IR must have at least one node".into(),
            });
        }
        Ok(ir)
    }
}

pub struct ModelResolutionPass {
    pub model_catalog: ModelCatalog,
}

impl ModelResolutionPass {
    pub fn new(model_catalog: ModelCatalog) -> Self {
        Self { model_catalog }
    }

    pub fn select_model(&self) -> &str {
        // Default to fast model when no requirements are given
        &self.model_catalog.fast
    }
}

#[async_trait::async_trait]
impl CompilerPass for ModelResolutionPass {
    fn name(&self) -> &str { "model_resolution" }

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
                        node.model = Some(self.select_model().to_string());
                    }
                }
            }
        }
        Ok(ir)
    }
}

pub struct ControlFlowValidationPass;

#[async_trait::async_trait]
impl CompilerPass for ControlFlowValidationPass {
    fn name(&self) -> &str { "control_flow_validation" }

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        let node_ids: HashSet<uuid::Uuid> = ir.nodes.iter().map(|n| n.id).collect();

        // Validate edge references
        for edge in &ir.edges {
            if !node_ids.contains(&edge.from) {
                return Err(CompilerError::ValidationError {
                    pass: "control_flow_validation".into(),
                    node_id: None,
                    message: format!("Edge from {} references unknown source node", edge.from),
                });
            }
            if !node_ids.contains(&edge.to) {
                return Err(CompilerError::ValidationError {
                    pass: "control_flow_validation".into(),
                    node_id: None,
                    message: format!("Edge to {} references unknown target node", edge.to),
                });
            }
        }

        // Validate per-kind invariants
        for node in &ir.nodes {
            match node.kind {
                IRNodeKind::Conditional => {
                    let outgoing: Vec<&IREdge> = ir.edges.iter()
                        .filter(|e| e.from == node.id)
                        .collect();
                    if outgoing.is_empty() {
                        return Err(CompilerError::ValidationError {
                            pass: "control_flow_validation".into(),
                            node_id: Some(node.id),
                            message: "Conditional node must have at least one outgoing edge".into(),
                        });
                    }
                    if !outgoing.iter().any(|e| e.condition.is_some()) {
                        return Err(CompilerError::ValidationError {
                            pass: "control_flow_validation".into(),
                            node_id: Some(node.id),
                            message: "Conditional node must have at least one edge with a condition".into(),
                        });
                    }
                }
                IRNodeKind::Loop => {
                    let outgoing: Vec<&IREdge> = ir.edges.iter()
                        .filter(|e| e.from == node.id)
                        .collect();
                    if outgoing.is_empty() {
                        return Err(CompilerError::ValidationError {
                            pass: "control_flow_validation".into(),
                            node_id: Some(node.id),
                            message: "Loop node must have at least one outgoing edge".into(),
                        });
                    }
                    if !node.config.contains_key("max_iterations") {
                        return Err(CompilerError::ValidationError {
                            pass: "control_flow_validation".into(),
                            node_id: Some(node.id),
                            message: "Loop node must have max_iterations in config".into(),
                        });
                    }
                }
                IRNodeKind::Split => {
                    let outgoing: Vec<&IREdge> = ir.edges.iter()
                        .filter(|e| e.from == node.id)
                        .collect();
                    if outgoing.len() < 2 {
                        return Err(CompilerError::ValidationError {
                            pass: "control_flow_validation".into(),
                            node_id: Some(node.id),
                            message: format!("Split node must have at least 2 outgoing edges, got {}", outgoing.len()),
                        });
                    }
                }
                IRNodeKind::Join => {
                    let incoming: Vec<&IREdge> = ir.edges.iter()
                        .filter(|e| e.to == node.id)
                        .collect();
                    if incoming.len() < 2 {
                        return Err(CompilerError::ValidationError {
                            pass: "control_flow_validation".into(),
                            node_id: Some(node.id),
                            message: format!("Join node must have at least 2 incoming edges, got {}", incoming.len()),
                        });
                    }
                }
                IRNodeKind::Barrier => {
                    let outgoing: Vec<&IREdge> = ir.edges.iter()
                        .filter(|e| e.from == node.id)
                        .collect();
                    let incoming: Vec<&IREdge> = ir.edges.iter()
                        .filter(|e| e.to == node.id)
                        .collect();
                    if incoming.is_empty() {
                        return Err(CompilerError::ValidationError {
                            pass: "control_flow_validation".into(),
                            node_id: Some(node.id),
                            message: "Barrier node must have at least one incoming edge".into(),
                        });
                    }
                    if outgoing.is_empty() {
                        return Err(CompilerError::ValidationError {
                            pass: "control_flow_validation".into(),
                            node_id: Some(node.id),
                            message: "Barrier node must have at least one outgoing edge".into(),
                        });
                    }
                }
                _ => {}
            }
        }

        // Detect illegal cycles (3-color DFS, skip loop back-edges)
        detect_illegal_cycles(&ir)?;

        Ok(ir)
    }
}

fn detect_illegal_cycles(ir: &WorkflowIR) -> Result<(), CompilerError> {
    let edges: Vec<(uuid::Uuid, uuid::Uuid)> = ir.edges.iter()
        .filter(|e| e.condition.as_deref() != Some("loop"))
        .map(|e| (e.from, e.to))
        .collect();

    match three_color_cycle_detect(&edges) {
        Ok(()) => Ok(()),
        Err(node_id) => Err(CompilerError::ValidationError {
            pass: "control_flow_validation".into(),
            node_id: Some(node_id),
            message: "Illegal cycle detected outside of loop back-edges".into(),
        }),
    }
}

fn three_color_cycle_detect(edges: &[(uuid::Uuid, uuid::Uuid)]) -> Result<(), uuid::Uuid> {
    #[derive(Clone, Copy, PartialEq)]
    enum Color { White, Grey, Black }

    let mut colors: HashMap<uuid::Uuid, Color> = HashMap::new();
    let mut graph: HashMap<uuid::Uuid, Vec<uuid::Uuid>> = HashMap::new();
    for (from, to) in edges {
        graph.entry(*from).or_default().push(*to);
        graph.entry(*to).or_default();
    }

    fn dfs(
        node: uuid::Uuid,
        graph: &HashMap<uuid::Uuid, Vec<uuid::Uuid>>,
        colors: &mut HashMap<uuid::Uuid, Color>,
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

// ---------------------------------------------------------------------------
// Lowering: WorkflowIR → ExecutionGraph
// ---------------------------------------------------------------------------

pub fn lower_to_graph(ir: WorkflowIR) -> Result<ExecutionGraph, CompilerError> {
    let mut exec_nodes = Vec::new();
    let mut exec_edges = Vec::new();

    for ir_node in &ir.nodes {
        exec_nodes.push(ExecutionNode {
            id: ir_node.id,
            kind: match ir_node.kind {
                IRNodeKind::Generate => ExecutionNodeKind::LLMGenerate,
                IRNodeKind::Review => ExecutionNodeKind::LLMReview,
                IRNodeKind::Judge => ExecutionNodeKind::LLMJudge,
                IRNodeKind::Transform => ExecutionNodeKind::Transform,
                IRNodeKind::Gate => ExecutionNodeKind::Gate,
                IRNodeKind::Conditional => ExecutionNodeKind::Conditional,
                IRNodeKind::Loop => ExecutionNodeKind::Loop,
                IRNodeKind::Split => ExecutionNodeKind::Split,
                IRNodeKind::Join => ExecutionNodeKind::Join,
                IRNodeKind::Barrier => ExecutionNodeKind::Barrier,
            },
            strategy: ir_node.strategy.clone(),
            model: ir_node.model.clone().unwrap_or_default(),
            retry_policy: RetryPolicy {
                max_retries: 2,
                backoff_ms: 1000,
            },
            fallback: None,
            config: ir_node.config.clone(),
            subgraph: None,
        });
    }

    for ir_edge in &ir.edges {
        exec_edges.push(ExecutionEdge {
            from: ir_edge.from,
            to: ir_edge.to,
            condition: ir_edge.condition.clone(),
        });
    }

    let total_cost = (ir.metadata.estimated_cost * 1000.0) as u64;
    let total_tokens = ir.metadata.estimated_tokens;

    Ok(ExecutionGraph {
        graph_id: ir.plan_id,
        nodes: exec_nodes,
        edges: exec_edges,
        metadata: GraphMetadata {
            estimated_cost: ir.metadata.estimated_cost,
            estimated_tokens: ir.metadata.estimated_tokens,
            max_depth: 1,
            node_count: ir.nodes.len() as u32,
        },
        primitive_graph_hash: 0,
        total_tokens,
        total_cost,
    })
}

// ---------------------------------------------------------------------------
// Compiler engine
// ---------------------------------------------------------------------------

pub struct CompilerEngine {
    passes: Vec<Box<dyn CompilerPass>>,
    resource_manager: Arc<dyn fusion_kernel::resource::ResourceManager>,
}

impl CompilerEngine {
    pub fn new() -> Self {
        Self::with_resource_manager(Arc::new(StubResourceManager::new(f64::INFINITY, u64::MAX)))
    }

    pub fn with_resource_manager(resource_manager: Arc<dyn fusion_kernel::resource::ResourceManager>) -> Self {
        let passes: Vec<Box<dyn CompilerPass>> = vec![
            Box::new(ConstraintValidationPass),
            Box::new(ControlFlowValidationPass),
            Box::new(ModelResolutionPass::new(ModelCatalog::default())),
            Box::new(BudgetOptimisationPass { resource_manager: resource_manager.clone() }),
        ];
        Self { passes, resource_manager }
    }

    pub fn with_model_catalog(model_catalog: ModelCatalog) -> Self {
        let rm: Arc<dyn fusion_kernel::resource::ResourceManager> = Arc::new(StubResourceManager::new(f64::INFINITY, u64::MAX));
        let passes: Vec<Box<dyn CompilerPass>> = vec![
            Box::new(ConstraintValidationPass),
            Box::new(ControlFlowValidationPass),
            Box::new(ModelResolutionPass::new(model_catalog)),
            Box::new(BudgetOptimisationPass { resource_manager: rm.clone() }),
        ];
        Self { passes, resource_manager: rm }
    }

    pub async fn compile(&self, intent: &str, ir: &WorkflowIR) -> Result<CompilerReport, PlatformError> {
        if intent.is_empty() {
            return Err(PlatformError::Compiler {
                code: "EMPTY_INTENT".to_string(),
                message: "Compiler intent cannot be empty".to_string(),
                recovery_suggestion: "Provide valid intent string".to_string(),
            });
        }

        let compile_start = std::time::Instant::now();
        let mut current_ir = ir.clone();
        let mut pass_names = Vec::new();
        let mut pass_diffs = Vec::new();

        for (idx, pass) in self.passes.iter().enumerate() {
            let pass_name = pass.name().to_string();
            let input_count = current_ir.nodes.len();
            current_ir = pass.apply(current_ir).await.map_err(|e| {
                PlatformError::Compiler {
                    code: "PASS_ERROR".to_string(),
                    message: format!("Pass '{}' failed: {}", pass_name, e),
                    recovery_suggestion: "Check IR validity and pass constraints".to_string(),
                }
            })?;
            let output_count = current_ir.nodes.len();

            pass_names.push(pass_name.clone());
            pass_diffs.push(CompilerPassDiff {
                pass_number: idx + 1,
                pass_name: pass_name.clone(),
                input_nodes: input_count,
                output_nodes: output_count,
                transformation_summary: format!("Executed pass {pass_name}"),
            });
        }

        let compilation_time_ms = compile_start.elapsed().as_millis() as u64;

        let route_scores = vec![
            self.explain_route("openrouter"),
            self.explain_route("zen"),
            self.explain_route("ollama"),
        ];

        let provider_comparison = Self::build_provider_comparison(&route_scores);

        Ok(CompilerReport {
            intent: intent.to_string(),
            passes_executed: pass_names,
            pass_diffs,
            graph_id: format!("graph_{}", ir.plan_id),
            compilation_time_ms,
            route_scores,
            provider_comparison,
        })
    }

    /// Compile the IR through all passes, then lower to an ExecutionGraph.
    /// Returns both the compiler report and the execution graph.
    pub async fn compile_and_lower(
        &self,
        intent: &str,
        ir: &WorkflowIR,
    ) -> Result<(CompilerReport, ExecutionGraph), PlatformError> {
        if intent.is_empty() {
            return Err(PlatformError::Compiler {
                code: "EMPTY_INTENT".to_string(),
                message: "Compiler intent cannot be empty".to_string(),
                recovery_suggestion: "Provide valid intent string".to_string(),
            });
        }

        let compile_start = std::time::Instant::now();
        let mut current_ir = ir.clone();
        let mut pass_names = Vec::new();
        let mut pass_diffs = Vec::new();

        for (idx, pass) in self.passes.iter().enumerate() {
            let pass_name = pass.name().to_string();
            let input_count = current_ir.nodes.len();
            current_ir = pass.apply(current_ir).await.map_err(|e| {
                PlatformError::Compiler {
                    code: "PASS_ERROR".to_string(),
                    message: format!("Pass '{}' failed: {}", pass_name, e),
                    recovery_suggestion: "Check IR validity and pass constraints".to_string(),
                }
            })?;
            let output_count = current_ir.nodes.len();

            pass_names.push(pass_name.clone());
            pass_diffs.push(CompilerPassDiff {
                pass_number: idx + 1,
                pass_name: pass_name.clone(),
                input_nodes: input_count,
                output_nodes: output_count,
                transformation_summary: format!("Executed pass {pass_name}"),
            });
        }

        let compilation_time_ms = compile_start.elapsed().as_millis() as u64;

        let route_scores = vec![
            self.explain_route("openrouter"),
            self.explain_route("zen"),
            self.explain_route("ollama"),
        ];

        let provider_comparison = Self::build_provider_comparison(&route_scores);

        let graph = lower_to_graph(current_ir).map_err(|e| PlatformError::Compiler {
            code: "LOWER_ERROR".to_string(),
            message: format!("Failed to lower IR to graph: {}", e),
            recovery_suggestion: "Check IR validity".to_string(),
        })?;

        Ok((CompilerReport {
            intent: intent.to_string(),
            passes_executed: pass_names,
            pass_diffs,
            graph_id: format!("graph_{}", ir.plan_id),
            compilation_time_ms,
            route_scores,
            provider_comparison,
        }, graph))
    }

    pub fn explain_route(&self, provider_name: &str) -> ExplainRouteScore {
        let capability_score: Option<f64> = None;
        let budget_score: Option<f64> = Some(1.0);
        let latency_score: Option<f64> = None;
        let health_score: Option<f64> = None;
        let policy_score: Option<f64> = None;

        let total_score = compute_total_score(
            capability_score, WEIGHT_CAPABILITY,
            budget_score, WEIGHT_BUDGET,
            latency_score, WEIGHT_LATENCY,
            health_score, WEIGHT_HEALTH,
            policy_score, WEIGHT_POLICY,
        );

        ExplainRouteScore {
            provider_name: provider_name.to_string(),
            capability_score,
            budget_score,
            latency_score,
            health_score,
            policy_score,
            total_score,
        }
    }

    fn build_provider_comparison(scores: &[ExplainRouteScore]) -> Vec<ProviderComparisonCandidate> {
        if scores.is_empty() {
            return Vec::new();
        }

        let mut ranked: Vec<&ExplainRouteScore> = scores.iter().collect();
        ranked.sort_by(|a, b| b.total_score.partial_cmp(&a.total_score).unwrap_or(std::cmp::Ordering::Equal));

        let best_score = ranked[0].total_score;
        let tied_at_top = ranked.iter()
            .filter(|s| s.total_score == best_score)
            .count();
        let unique_best = tied_at_top == 1;

        ranked.into_iter().enumerate().map(|(i, score)| {
            let status = if score.total_score == 0.0 {
                "Unscored"
            } else if i == 0 && unique_best {
                "Selected"
            } else if score.total_score >= best_score * 0.9 {
                "Alternative"
            } else {
                "Filtered"
            };

            let reason = if score.total_score == 0.0 {
                "Score not computed — all sub-scores are None".to_string()
            } else if i == 0 && unique_best {
                format!("Highest total score ({:.2}) among evaluated providers", score.total_score)
            } else if i == 0 && !unique_best {
                format!(
                    "Tied with {} other provider(s) at {:.2} — selected by position",
                    tied_at_top - 1, score.total_score
                )
            } else {
                let delta = best_score - score.total_score;
                if delta < 0.05 {
                    format!("Close alternative ({:.2} vs {:.2}, delta {:.2})", score.total_score, best_score, delta)
                } else {
                    format!("Lower total score ({:.2} vs {:.2}, delta {:.2})", score.total_score, best_score, delta)
                }
            };

            let model_name = match score.provider_name.as_str() {
                "openrouter" => "claude-3-5-sonnet",
                "zen" => "gemini-2.5-pro",
                "ollama" => "llama3",
                _ => "unknown",
            };

            ProviderComparisonCandidate {
                provider_name: score.provider_name.clone(),
                model_name: model_name.to_string(),
                total_score: score.total_score,
                status: status.to_string(),
                reason,
            }
        }).collect()
    }
}

impl Default for CompilerEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Budget pass (delegates to resource manager)
// ---------------------------------------------------------------------------

pub struct BudgetOptimisationPass {
    pub resource_manager: Arc<dyn fusion_kernel::resource::ResourceManager>,
}

#[async_trait::async_trait]
impl CompilerPass for BudgetOptimisationPass {
    fn name(&self) -> &str { "budget_optimisation" }

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        if !self.resource_manager.can_afford(ir.metadata.estimated_cost, ir.metadata.estimated_tokens).await {
            return Err(CompilerError::ValidationError {
                pass: "budget_optimisation".into(),
                node_id: None,
                message: "Budget exceeded".into(),
            });
        }
        Ok(ir)
    }
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

fn compute_total_score(
    cap: Option<f64>, w_cap: f64,
    bud: Option<f64>, w_bud: f64,
    lat: Option<f64>, w_lat: f64,
    hea: Option<f64>, w_hea: f64,
    pol: Option<f64>, w_pol: f64,
) -> f64 {
    let pairs: &[(Option<f64>, f64)] = &[
        (cap, w_cap), (bud, w_bud), (lat, w_lat), (hea, w_hea), (pol, w_pol),
    ];

    let total_weight: f64 = pairs.iter()
        .filter_map(|(score, weight)| score.map(|_| weight))
        .sum();

    if total_weight == 0.0 {
        return 0.0;
    }

    let weighted_sum: f64 = pairs.iter()
        .filter_map(|(score, weight)| score.map(|s| s * weight))
        .sum();

    weighted_sum / total_weight
}

// ---------------------------------------------------------------------------
// Default compiler (convenience)
// ---------------------------------------------------------------------------

pub struct DefaultCompiler {
    engine: CompilerEngine,
}

impl DefaultCompiler {
    pub fn new() -> Self {
        Self { engine: CompilerEngine::new() }
    }
}

impl Default for DefaultCompiler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Compiler for DefaultCompiler {
    async fn compile(&self, ir: WorkflowIR) -> Result<ExecutionGraph, CompilerError> {
        let report = self.engine.compile("Default Compilation", &ir).await
            .map_err(|e| CompilerError::PassError {
                pass: "compile".into(),
                message: e.to_string(),
            })?;
        lower_to_graph(ir)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ir() -> WorkflowIR {
        WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![IRNode {
                id: uuid::Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: None,
                config: HashMap::new(),
            }],
            edges: vec![],
            metadata: IRMetadata {
                policy_applied: vec![],
                estimated_cost: 0.1,
                estimated_tokens: 500,
            },
        }
    }

    #[tokio::test]
    async fn test_constraint_validation_rejects_empty() {
        let pass = ConstraintValidationPass;
        let empty_ir = WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![],
            edges: vec![],
            metadata: IRMetadata {
                policy_applied: vec![],
                estimated_cost: 0.0,
                estimated_tokens: 0,
            },
        };
        let result = pass.apply(empty_ir).await;
        assert!(result.is_err());
        match result {
            Err(CompilerError::ValidationError { pass: p, .. }) => assert_eq!(p, "constraint_validation"),
            _ => panic!("Expected ValidationError"),
        }
    }

    #[tokio::test]
    async fn test_constraint_validation_accepts_nonempty() {
        let pass = ConstraintValidationPass;
        let ir = test_ir();
        assert!(pass.apply(ir).await.is_ok());
    }

    #[tokio::test]
    async fn test_model_resolution_fills_missing_models() {
        let pass = ModelResolutionPass::new(ModelCatalog {
            fast: "fast-model".into(),
            ..Default::default()
        });
        let ir = test_ir();
        let result = pass.apply(ir).await.unwrap();
        assert_eq!(result.nodes[0].model.as_deref(), Some("fast-model"));
    }

    #[tokio::test]
    async fn test_model_resolution_preserves_existing_model() {
        let pass = ModelResolutionPass::new(ModelCatalog {
            fast: "fast-model".into(),
            ..Default::default()
        });
        let mut ir = test_ir();
        ir.nodes[0].model = Some("custom-model".into());
        let result = pass.apply(ir).await.unwrap();
        assert_eq!(result.nodes[0].model.as_deref(), Some("custom-model"));
    }

    #[tokio::test]
    async fn test_model_resolution_skips_control_flow_nodes() {
        let pass = ModelResolutionPass::new(ModelCatalog {
            fast: "fast-model".into(),
            ..Default::default()
        });
        let mut ir = test_ir();
        ir.nodes[0].kind = IRNodeKind::Conditional;
        let result = pass.apply(ir).await.unwrap();
        assert_eq!(result.nodes[0].model, None);
    }

    #[tokio::test]
    async fn test_control_flow_validation_rejects_dangling_edge() {
        let pass = ControlFlowValidationPass;
        let mut ir = test_ir();
        ir.edges.push(IREdge {
            from: uuid::Uuid::new_v4(),
            to: uuid::Uuid::new_v4(),
            condition: None,
        });
        let result = pass.apply(ir).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_control_flow_validation_rejects_conditional_without_condition() {
        let pass = ControlFlowValidationPass;
        let mut ir = test_ir();
        let node_id = ir.nodes[0].id;
        ir.nodes[0].kind = IRNodeKind::Conditional;
        ir.edges.push(IREdge {
            from: node_id,
            to: uuid::Uuid::new_v4(),
            condition: None,
        });
        // Add the target node
        ir.nodes.push(IRNode {
            id: ir.edges[0].to,
            kind: IRNodeKind::Generate,
            strategy: StrategyKind::Single,
            model: None,
            config: HashMap::new(),
        });
        let result = pass.apply(ir).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_lower_to_graph_produces_execution_graph() {
        let ir = test_ir();
        let graph = lower_to_graph(ir.clone()).unwrap();
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.edges.len(), 0);
        assert_eq!(graph.graph_id, ir.plan_id);
        assert_eq!(graph.nodes[0].kind, ExecutionNodeKind::LLMGenerate);
        assert_eq!(graph.nodes[0].retry_policy.max_retries, 2);
    }

    #[tokio::test]
    async fn test_compiler_engine_pass_pipeline() {
        let engine = CompilerEngine::new();
        let ir = test_ir();
        let report = engine.compile("Code Generation", &ir).await.expect("Compile");
        assert_eq!(report.passes_executed.len(), 4);
        assert_eq!(report.pass_diffs.len(), 4);
    }

    #[test]
    fn test_total_score_computed_from_available_scores() {
        let engine = CompilerEngine::new();
        let score = engine.explain_route("openrouter");
        assert_eq!(score.provider_name, "openrouter");
        assert_eq!(score.budget_score, Some(1.0));
        assert_eq!(score.total_score, 1.0);
    }

    #[test]
    fn test_total_score_zero_when_all_none() {
        let total = compute_total_score(None, 0.3, None, 0.25, None, 0.2, None, 0.15, None, 0.1);
        assert_eq!(total, 0.0);
    }

    #[test]
    fn test_total_score_renormalizes_weights() {
        let total = compute_total_score(Some(0.8), 0.3, Some(0.6), 0.2, None, 0.2, None, 0.15, None, 0.1);
        assert!((total - 0.72).abs() < 1e-10);
    }

    #[test]
    fn test_provider_comparison_sorted_by_score() {
        let engine = CompilerEngine::new();
        let scores = vec![
            engine.explain_route("ollama"),
            engine.explain_route("openrouter"),
            engine.explain_route("zen"),
        ];
        let comparison = CompilerEngine::build_provider_comparison(&scores);
        assert_eq!(comparison.len(), 3);
        assert_eq!(comparison[0].status, "Alternative");
        assert!(comparison[0].reason.contains("Tied with 2 other provider(s) at 1.00"));
    }

    #[test]
    fn test_provider_comparison_alternative_status() {
        let scores = vec![
            ExplainRouteScore {
                provider_name: "a".into(),
                capability_score: None,
                budget_score: Some(1.0),
                latency_score: None,
                health_score: None,
                policy_score: None,
                total_score: 1.0,
            },
            ExplainRouteScore {
                provider_name: "b".into(),
                capability_score: None,
                budget_score: Some(0.95),
                latency_score: None,
                health_score: None,
                policy_score: None,
                total_score: 0.95,
            },
            ExplainRouteScore {
                provider_name: "c".into(),
                capability_score: None,
                budget_score: Some(0.5),
                latency_score: None,
                health_score: None,
                policy_score: None,
                total_score: 0.5,
            },
        ];
        let comparison = CompilerEngine::build_provider_comparison(&scores);
        assert_eq!(comparison[0].status, "Selected");
        assert_eq!(comparison[1].status, "Alternative");
        assert_eq!(comparison[2].status, "Filtered");
    }

    #[tokio::test]
    async fn test_budget_pass_under_quota() {
        let rm = Arc::new(StubResourceManager::new(f64::INFINITY, u64::MAX));
        let pass = BudgetOptimisationPass { resource_manager: rm };
        let ir = test_ir();
        assert!(pass.apply(ir).await.is_ok());
    }

    #[tokio::test]
    async fn test_budget_pass_over_quota() {
        let rm = Arc::new(StubResourceManager::new(0.0, 0));
        let pass = BudgetOptimisationPass { resource_manager: rm };
        let ir = test_ir();
        let result = pass.apply(ir).await;
        assert!(result.is_err());
    }
}
