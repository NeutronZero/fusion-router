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
pub mod policy;
pub mod score;
pub mod strategy_compiler;
pub mod strategy_expansion;
pub mod content_hash;

use strategy_compiler::StrategyLoweringPass;

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
    pub duration_ms: u64,
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
    async fn compile(&self, ir: &WorkflowIR) -> Result<ExecutionGraph, CompilerError>;
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
    lower_to_graph_with_compilers(ir, &std::collections::HashMap::new())
}

/// Lower a `WorkflowIR` to an `ExecutionGraph`, using registered custom
/// strategy compilers for `Custom` nodes. This is the structurally-mandatory
/// entry point — `Custom` strategies require a registered delegate.
pub fn lower_to_graph_with_compilers(
    ir: WorkflowIR,
    custom_compilers: &std::collections::HashMap<String, std::sync::Arc<dyn strategy_compiler::StrategyCompiler>>,
) -> Result<ExecutionGraph, CompilerError> {
    let mut exec_nodes = Vec::new();
    let mut exec_edges = Vec::new();

    for ir_node in &ir.nodes {
        let mut node = ExecutionNode {
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
        };
        // Phase 3.5: attach prebuilt subgraph for non-Single strategies so the
        // Phase 4 runtime subgraph path is the production path for Consensus.
        // Custom strategies require a registered delegate compiler.
        node.subgraph = strategy_expansion::expanded_subgraph_with_compilers(&node, custom_compilers);
        exec_nodes.push(node);
    }

    for ir_edge in &ir.edges {
        exec_edges.push(ExecutionEdge {
            from: ir_edge.from,
            to: ir_edge.to,
            condition: ir_edge.condition.clone(),
        });
    }

    let total_cost = ir.metadata.estimated_cost;
    let total_tokens = ir.metadata.estimated_tokens;

    Ok(ExecutionGraph {
        graph_id: ir.plan_id,
        nodes: exec_nodes,
        edges: exec_edges,
        metadata: GraphMetadata {
            estimated_cost: ir.metadata.estimated_cost,
            estimated_tokens: ir.metadata.estimated_tokens,
            policy_version: ir.metadata.policy_version,
            max_depth: 1,
            node_count: ir.nodes.len() as u32,
        },
        primitive_graph_hash: content_hash::compute_workflow_content_hash(&ir),
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
    score_sources: score::ScoreSources,
    custom_compilers: std::collections::HashMap<String, std::sync::Arc<dyn strategy_compiler::StrategyCompiler>>,
}

impl CompilerEngine {
    pub fn new() -> Self {
        Self::with_resource_manager(Arc::new(StubResourceManager::new(fusion_kernel::resource::Quota { max_daily_cost: fusion_core::NanoUSD::from_nanos(u64::MAX), max_daily_tokens: u64::MAX })))
    }

    pub fn with_resource_manager(resource_manager: Arc<dyn fusion_kernel::resource::ResourceManager>) -> Self {
        let passes: Vec<Box<dyn CompilerPass>> = vec![
            Box::new(ConstraintValidationPass),
            Box::new(ControlFlowValidationPass),
            Box::new(DeadNodeEliminationPass),
            Box::new(ModelResolutionPass::new(ModelCatalog::default())),
            Box::new(BudgetOptimisationPass { resource_manager: resource_manager.clone() }),
        ];
        Self { passes, resource_manager, score_sources: score::ScoreSources::default(), custom_compilers: std::collections::HashMap::new() }
    }

    pub fn with_model_catalog(model_catalog: ModelCatalog) -> Self {
        let rm: Arc<dyn fusion_kernel::resource::ResourceManager> = Arc::new(StubResourceManager::new(fusion_kernel::resource::Quota { max_daily_cost: fusion_core::NanoUSD::from_nanos(u64::MAX), max_daily_tokens: u64::MAX }));
        let passes: Vec<Box<dyn CompilerPass>> = vec![
            Box::new(ConstraintValidationPass),
            Box::new(ControlFlowValidationPass),
            Box::new(DeadNodeEliminationPass),
            Box::new(ModelResolutionPass::new(model_catalog)),
            Box::new(BudgetOptimisationPass { resource_manager: rm.clone() }),
        ];
        Self { passes, resource_manager: rm, score_sources: score::ScoreSources::default(), custom_compilers: std::collections::HashMap::new() }
    }

    /// Creates an engine with an empty pass list and the given resource manager.
    pub fn with_resource_manager_custom(resource_manager: Arc<dyn fusion_kernel::resource::ResourceManager>) -> Self {
        Self { passes: Vec::new(), resource_manager, score_sources: score::ScoreSources::default(), custom_compilers: std::collections::HashMap::new() }
    }

    /// Register a custom strategy compiler delegate. The compiler is used during
    /// `lower_to_graph` to expand `Custom` strategy nodes.
    pub fn register_custom_compiler(mut self, name: impl Into<String>, compiler: std::sync::Arc<dyn strategy_compiler::StrategyCompiler>) -> Self {
        self.custom_compilers.insert(name.into(), compiler);
        self
    }

    /// Replaces the pluggable route scorers (defaults are static/offline).
    pub fn with_score_sources(mut self, score_sources: score::ScoreSources) -> Self {
        self.score_sources = score_sources;
        self
    }

    /// Appends a pass to the pipeline.
    pub fn add_pass(&mut self, pass: Box<dyn CompilerPass>) {
        self.passes.push(pass);
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
            let pass_start = std::time::Instant::now();
            current_ir = pass.apply(current_ir).await.map_err(|e| {
                PlatformError::Compiler {
                    code: "PASS_ERROR".to_string(),
                    message: format!("Pass '{}' failed: {}", pass_name, e),
                    recovery_suggestion: "Check IR validity and pass constraints".to_string(),
                }
            })?;
            let output_count = current_ir.nodes.len();
            let duration_ms = pass_start.elapsed().as_millis() as u64;

            pass_names.push(pass_name.clone());
            pass_diffs.push(CompilerPassDiff {
                pass_number: idx + 1,
                pass_name: pass_name.clone(),
                input_nodes: input_count,
                output_nodes: output_count,
                transformation_summary: format!("Executed pass {pass_name}"),
                duration_ms,
            });
        }

        let compilation_time_ms = compile_start.elapsed().as_millis() as u64;

        let route_scores = vec![
            self.explain_route("openrouter", intent, &current_ir).await,
            self.explain_route("zen", intent, &current_ir).await,
            self.explain_route("ollama", intent, &current_ir).await,
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
            let pass_start = std::time::Instant::now();
            current_ir = pass.apply(current_ir).await.map_err(|e| {
                PlatformError::Compiler {
                    code: "PASS_ERROR".to_string(),
                    message: format!("Pass '{}' failed: {}", pass_name, e),
                    recovery_suggestion: "Check IR validity and pass constraints".to_string(),
                }
            })?;
            let output_count = current_ir.nodes.len();
            let duration_ms = pass_start.elapsed().as_millis() as u64;

            pass_names.push(pass_name.clone());
            pass_diffs.push(CompilerPassDiff {
                pass_number: idx + 1,
                pass_name: pass_name.clone(),
                input_nodes: input_count,
                output_nodes: output_count,
                transformation_summary: format!("Executed pass {pass_name}"),
                duration_ms,
            });
        }

        let compilation_time_ms = compile_start.elapsed().as_millis() as u64;

        let route_scores = vec![
            self.explain_route("openrouter", intent, &current_ir).await,
            self.explain_route("zen", intent, &current_ir).await,
            self.explain_route("ollama", intent, &current_ir).await,
        ];

        let provider_comparison = Self::build_provider_comparison(&route_scores);

        let graph = lower_to_graph_with_compilers(current_ir, &self.custom_compilers).map_err(|e| PlatformError::Compiler {
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

    /// Computes the multi-dimensional route score for a provider.
    ///
    /// Each sub-score comes from the pluggable `ScoreSources` (static and
    /// offline by default); a missing scorer contributes `None` and
    /// `compute_total_score` renormalizes over the available weights. The
    /// budget score is `1.0` when the resource manager can afford the IR.
    pub async fn explain_route(
        &self,
        provider_name: &str,
        intent: &str,
        ir: &WorkflowIR,
    ) -> ExplainRouteScore {
        let capability_score = match &self.score_sources.capability {
            Some(scorer) => scorer.score(provider_name, intent).await,
            None => None,
        };
        let health_score = match &self.score_sources.health {
            Some(scorer) => scorer.score(provider_name).await,
            None => None,
        };
        let latency_score = match &self.score_sources.latency {
            Some(scorer) => scorer.score(provider_name).await,
            None => None,
        };
        let policy_score = match &self.score_sources.policy {
            Some(scorer) => scorer.score(provider_name, ir).await,
            None => None,
        };
        let budget_score = if self.resource_manager.can_afford(
            fusion_core::NanoUSD::checked_from_decimal_usd(&format!("{:.9}", ir.metadata.estimated_cost)).unwrap_or(fusion_core::NanoUSD::ZERO),
            ir.metadata.estimated_tokens,
        ).await {
            Some(1.0)
        } else {
            Some(0.0)
        };

        let total_score = compute_total_score(&[
            (capability_score, WEIGHT_CAPABILITY),
            (budget_score, WEIGHT_BUDGET),
            (latency_score, WEIGHT_LATENCY),
            (health_score, WEIGHT_HEALTH),
            (policy_score, WEIGHT_POLICY),
        ]);

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
        if !self.resource_manager.can_afford(
            ir.metadata.estimated_cost,
            ir.metadata.estimated_tokens,
        ).await {
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
// Dead-node elimination (Phase 3.3)
// ---------------------------------------------------------------------------

/// Removes nodes unreachable from any edge source.
///
/// **Root semantics:** roots are nodes that appear as `from` in at least one
/// edge — NOT "nodes with no incoming edges." This means isolated nodes (no
/// incoming or outgoing edges) are always eliminated when the graph has edges.
/// Edgeless graphs (single-node templates, etc.) keep all nodes unchanged.
pub struct DeadNodeEliminationPass;

#[async_trait::async_trait]
impl CompilerPass for DeadNodeEliminationPass {
    fn name(&self) -> &str { "dead_node_elimination" }

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        if ir.nodes.is_empty() {
            return Ok(ir);
        }

        // If there are no edges, keep all nodes (nothing can be unreachable)
        if ir.edges.is_empty() {
            return Ok(ir);
        }

        // Roots = nodes that are sources of at least one edge.
        // Isolated nodes (no incoming or outgoing edges) are NOT roots and will
        // be eliminated when the graph has edges.
        let roots: HashSet<uuid::Uuid> = ir.edges.iter().map(|e| e.from).collect();

        // BFS from roots to find reachable nodes
        let mut reachable: HashSet<uuid::Uuid> = HashSet::new();
        let mut queue: std::collections::VecDeque<uuid::Uuid> = roots.into_iter().collect();
        while let Some(current) = queue.pop_front() {
            if !reachable.insert(current) {
                continue;
            }
            for edge in &ir.edges {
                if edge.from == current && !reachable.contains(&edge.to) {
                    queue.push_back(edge.to);
                }
            }
        }

        // Filter nodes and edges
        let live_nodes: Vec<IRNode> = ir.nodes.into_iter()
            .filter(|n| reachable.contains(&n.id))
            .collect();
        let live_edges: Vec<IREdge> = ir.edges.into_iter()
            .filter(|e| reachable.contains(&e.from) && reachable.contains(&e.to))
            .collect();

        Ok(WorkflowIR {
            plan_id: ir.plan_id,
            nodes: live_nodes,
            edges: live_edges,
            metadata: ir.metadata,
        })
    }
}

// ---------------------------------------------------------------------------
// Policy pass (Phase 3)
// ---------------------------------------------------------------------------

pub struct PolicyCompilerPass {
    policy_ir: policy::PolicyIR,
}

impl PolicyCompilerPass {
    pub fn new(policy_ir: policy::PolicyIR) -> Self {
        Self { policy_ir }
    }
}

#[async_trait::async_trait]
impl CompilerPass for PolicyCompilerPass {
    fn name(&self) -> &str { "policy" }

    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError> {
        let mut new_nodes = ir.nodes.clone();
        let mut new_edges = ir.edges.clone();
        let mut trace = policy::PolicyTrace::new();
        let mut inserted_gate_nodes = Vec::new();

        for node in &ir.nodes {
            // Symbol key resolution: config["capability"] > model > "general"
            let symbol_key = node
                .config
                .get("capability")
                .and_then(|v| v.as_str())
                .or(node.model.as_deref())
                .unwrap_or("general");

            if let Some(rule) = policy::PolicyPrecedenceEngine::evaluate_matching_rule(&self.policy_ir, symbol_key) {
                trace.record(policy::PolicyMatchEvent::RuleMatched {
                    rule_id: rule.rule_id.clone(),
                    symbol: symbol_key.to_string(),
                    effect: rule.effect.clone(),
                });

                if rule.effect == policy::PolicyEffect::Deny {
                    return Err(CompilerError::ValidationError {
                        pass: "policy".into(),
                        node_id: Some(node.id),
                        message: format!(
                            "Policy rule '{}' denies target '{}' (effect: deny); node {} cannot be compiled",
                            rule.rule_id, rule.target_pattern, node.id
                        ),
                    });
                }

                if rule.effect == policy::PolicyEffect::Approval {
                    // Idempotence: skip if a Gate already guards this node
                    let already_guarded = new_edges.iter().any(|edge| {
                        edge.to == node.id
                            && new_nodes.iter().any(|n| n.id == edge.from && n.kind == IRNodeKind::Gate)
                    });

                    if !already_guarded {
                        let gate_id = uuid::Uuid::new_v5(&node.id, b"policy_approval_gate");
                        let gate_node = IRNode {
                            id: gate_id,
                            kind: IRNodeKind::Gate,
                            strategy: StrategyKind::Single,
                            model: Some("policy.approval_gate".into()),
                            config: std::collections::HashMap::new(),
                        };
                        trace.record(policy::PolicyMatchEvent::NodeInserted {
                            gate_id,
                            target_node_id: node.id,
                        });
                        inserted_gate_nodes.push(gate_node);
                        new_edges.push(IREdge {
                            from: gate_id,
                            to: node.id,
                            condition: None,
                        });
                    }
                }
            }
        }

        new_nodes.extend(inserted_gate_nodes);

        Ok(WorkflowIR {
            plan_id: ir.plan_id,
            nodes: new_nodes,
            edges: new_edges,
            metadata: ir.metadata,
        })
    }
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

fn compute_total_score(scored_metrics: &[(Option<f64>, f64)]) -> f64 {
    let total_weight: f64 = scored_metrics.iter()
        .filter_map(|(score, weight)| score.map(|_| *weight))
        .sum();

    if total_weight == 0.0 {
        return 0.0;
    }

    let weighted_sum: f64 = scored_metrics.iter()
        .filter_map(|(score, weight)| score.map(|s| s * *weight))
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
    async fn compile(&self, ir: &WorkflowIR) -> Result<ExecutionGraph, CompilerError> {
        let (_report, graph) = self.engine.compile_and_lower("Default Compilation", ir).await
            .map_err(|e| CompilerError::PassError {
                pass: "compile".into(),
                message: e.to_string(),
            })?;
        Ok(graph)
    }
}

// ---------------------------------------------------------------------------
// build_compiler factory (Phase 3.2)
// ---------------------------------------------------------------------------

/// Creates a `CompilerEngine` with the mandatory pass pipeline and an optional
/// policy pass appended at the end.
///
/// Mandatory order (without policy):
///   constraint_validation → control_flow_validation → strategy_lowering →
///   dead_node_elimination → model_resolution → budget_optimisation
///
/// When `policy_ir` is `Some`, the policy pass is appended last.
pub fn build_compiler(
    model_catalog: ModelCatalog,
    resource_manager: Arc<dyn fusion_kernel::resource::ResourceManager>,
    policy_ir: Option<policy::PolicyIR>,
) -> CompilerEngine {
    let mut engine = CompilerEngine::with_resource_manager_custom(resource_manager.clone());
    engine.add_pass(Box::new(ConstraintValidationPass));
    engine.add_pass(Box::new(ControlFlowValidationPass));
    engine.add_pass(Box::new(StrategyLoweringPass::new()));
    engine.add_pass(Box::new(DeadNodeEliminationPass));
    engine.add_pass(Box::new(ModelResolutionPass::new(model_catalog)));
    engine.add_pass(Box::new(BudgetOptimisationPass { resource_manager }));
    if let Some(ir) = policy_ir {
        engine.add_pass(Box::new(PolicyCompilerPass::new(ir)));
    }
    engine
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_core::NanoUSD;

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
                policy_version: 0,
                policy_applied: vec![],
                estimated_cost: NanoUSD::from_nanos(100_000_000),
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
                policy_version: 0,
                policy_applied: vec![],
                estimated_cost: NanoUSD::ZERO,
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
        assert_eq!(report.passes_executed.len(), 5);
        assert_eq!(report.pass_diffs.len(), 5);
        // duration_ms is unsigned; non-negativity is guaranteed by the type
    }

    #[tokio::test]
    async fn test_total_score_computed_from_available_scores() {
        let engine = CompilerEngine::new();
        let score = engine.explain_route("openrouter", "general question", &test_ir()).await;
        assert_eq!(score.provider_name, "openrouter");
        assert_eq!(score.budget_score, Some(1.0));
        assert!(score.capability_score.is_some(), "default engine must produce a capability score");
        assert!(score.health_score.is_some(), "default engine must produce a health score");
        // cap 0.9*0.3 + bud 1.0*0.25 + lat 0.6*0.2 + hea 1.0*0.15 + pol 1.0*0.1
        assert!((score.total_score - 0.89).abs() < 1e-9, "total was {}", score.total_score);
        assert!(score.total_score < 1.0, "multi-dimensional score must not tie at 1.0");
    }

    #[test]
    fn test_total_score_zero_when_all_none() {
        let total = compute_total_score(&[(None, 0.3), (None, 0.25), (None, 0.2), (None, 0.15), (None, 0.1)]);
        assert_eq!(total, 0.0);
    }

    #[test]
    fn test_total_score_renormalizes_weights() {
        let total = compute_total_score(&[(Some(0.8), 0.3), (Some(0.6), 0.2), (None, 0.2), (None, 0.15), (None, 0.1)]);
        assert!((total - 0.72).abs() < 1e-10);
    }

    #[tokio::test]
    async fn test_provider_comparison_sorted_by_score() {
        let engine = CompilerEngine::new();
        let ir = test_ir();
        let scores = vec![
            engine.explain_route("ollama", "general question", &ir).await,
            engine.explain_route("openrouter", "general question", &ir).await,
            engine.explain_route("zen", "general question", &ir).await,
        ];
        let comparison = CompilerEngine::build_provider_comparison(&scores);
        assert_eq!(comparison.len(), 3);
        // Default static tables differentiate: zen > openrouter > ollama
        assert_eq!(comparison[0].provider_name, "zen");
        assert_eq!(comparison[0].status, "Selected");
        assert_eq!(comparison[1].status, "Alternative");
        assert_eq!(comparison[2].status, "Filtered");
        assert!(
            comparison[0].total_score > comparison[1].total_score
                && comparison[1].total_score > comparison[2].total_score,
            "totals must be differentiated, not all tied at 1.0"
        );
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
        let rm = Arc::new(StubResourceManager::new(fusion_kernel::resource::Quota { max_daily_cost: NanoUSD::from_nanos(u64::MAX), max_daily_tokens: u64::MAX }));
        let pass = BudgetOptimisationPass { resource_manager: rm };
        let ir = test_ir();
        assert!(pass.apply(ir).await.is_ok());
    }

    #[tokio::test]
    async fn test_budget_pass_over_quota() {
        let rm = Arc::new(StubResourceManager::new(fusion_kernel::resource::Quota { max_daily_cost: NanoUSD::ZERO, max_daily_tokens: 0 }));
        let pass = BudgetOptimisationPass { resource_manager: rm };
        let ir = test_ir();
        let result = pass.apply(ir).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_model_resolution_fills_model_after_compile_and_lower() {
        let engine = CompilerEngine::with_model_catalog(ModelCatalog {
            code: "deepseek-chat".into(),
            fast: "gpt-4o-mini".into(),
            ..ModelCatalog::default()
        });
        let ir = WorkflowIR {
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
                policy_version: 0,
                policy_applied: vec![],
                estimated_cost: NanoUSD::from_nanos(10_000_000),
                estimated_tokens: 100,
            },
        };
        let (_report, graph) = engine.compile_and_lower("test", &ir).await.expect("compile_and_lower");
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].model, "gpt-4o-mini", "model_resolution must fill model from catalog");
    }

    // -----------------------------------------------------------------------
    // Phase 3: PolicyCompilerPass tests
    // -----------------------------------------------------------------------

    fn policy_ir_with_deny(target: &str) -> policy::PolicyIR {
        policy::PolicyIR {
            rules: vec![policy::PolicyRule {
                rule_id: "deny-shell".into(),
                target_pattern: target.into(),
                priority: 100,
                effect: policy::PolicyEffect::Deny,
                conditions: vec![],
                actions: vec![],
            }],
        }
    }

    fn policy_ir_with_approval(target: &str) -> policy::PolicyIR {
        policy::PolicyIR {
            rules: vec![policy::PolicyRule {
                rule_id: "approval-web".into(),
                target_pattern: target.into(),
                priority: 50,
                effect: policy::PolicyEffect::Approval,
                conditions: vec![],
                actions: vec![],
            }],
        }
    }

    #[tokio::test]
    async fn test_policy_deny_blocks_compilation() {
        let pass = PolicyCompilerPass::new(policy_ir_with_deny("shell.exec"));
        let mut config = HashMap::new();
        config.insert("capability".into(), serde_json::json!("shell.exec"));
        let ir = WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![IRNode {
                id: uuid::Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: None,
                config,
            }],
            edges: vec![],
            metadata: IRMetadata {
                policy_version: 0,
                policy_applied: vec![],
                estimated_cost: NanoUSD::ZERO,
                estimated_tokens: 0,
            },
        };
        let result = pass.apply(ir).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("deny"), "error should mention deny: {msg}");
    }

    #[tokio::test]
    async fn test_policy_approval_injects_gate() {
        let pass = PolicyCompilerPass::new(policy_ir_with_approval("web.fetch"));
        let mut config = HashMap::new();
        config.insert("capability".into(), serde_json::json!("web.fetch"));
        let node_id = uuid::Uuid::new_v4();
        let ir = WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![IRNode {
                id: node_id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: None,
                config,
            }],
            edges: vec![],
            metadata: IRMetadata {
                policy_version: 0,
                policy_applied: vec![],
                estimated_cost: NanoUSD::ZERO,
                estimated_tokens: 0,
            },
        };
        let result = pass.apply(ir).await.expect("pass should succeed");
        // Should have 2 nodes (original + gate) and 1 edge (gate -> original)
        assert_eq!(result.nodes.len(), 2);
        assert_eq!(result.edges.len(), 1);
        let gate = result.nodes.iter().find(|n| n.kind == IRNodeKind::Gate).unwrap();
        assert_eq!(result.edges[0].from, gate.id);
        assert_eq!(result.edges[0].to, node_id);
    }

    #[tokio::test]
    async fn test_policy_unrelated_not_denied() {
        let pass = PolicyCompilerPass::new(policy_ir_with_deny("shell.exec"));
        let mut config = HashMap::new();
        config.insert("capability".into(), serde_json::json!("web.fetch"));
        let ir = WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![IRNode {
                id: uuid::Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: None,
                config,
            }],
            edges: vec![],
            metadata: IRMetadata {
                policy_version: 0,
                policy_applied: vec![],
                estimated_cost: NanoUSD::ZERO,
                estimated_tokens: 0,
            },
        };
        let result = pass.apply(ir).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().nodes.len(), 1);
    }

    #[tokio::test]
    async fn test_policy_deny_outranks_approval() {
        // Deny on same target should win even if approval has higher priority
        let ir = policy::PolicyIR {
            rules: vec![
                policy::PolicyRule {
                    rule_id: "approval-high".into(),
                    target_pattern: "shell.exec".into(),
                    priority: 100,
                    effect: policy::PolicyEffect::Approval,
                    conditions: vec![],
                    actions: vec![],
                },
                policy::PolicyRule {
                    rule_id: "deny-low".into(),
                    target_pattern: "shell.exec".into(),
                    priority: 1,
                    effect: policy::PolicyEffect::Deny,
                    conditions: vec![],
                    actions: vec![],
                },
            ],
        };
        let pass = PolicyCompilerPass::new(ir);
        let mut config = HashMap::new();
        config.insert("capability".into(), serde_json::json!("shell.exec"));
        let ir = WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![IRNode {
                id: uuid::Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: None,
                config,
            }],
            edges: vec![],
            metadata: IRMetadata {
                policy_version: 0,
                policy_applied: vec![],
                estimated_cost: NanoUSD::ZERO,
                estimated_tokens: 0,
            },
        };
        let result = pass.apply(ir).await;
        assert!(result.is_err(), "deny should win over approval");
    }

    // -----------------------------------------------------------------------
    // Phase 3: DeadNodeEliminationPass tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_dead_node_elimination_removes_unreachable() {
        let pass = DeadNodeEliminationPass;
        let id_a = uuid::Uuid::new_v4();
        let id_b = uuid::Uuid::new_v4();
        let id_orphan = uuid::Uuid::new_v4();
        let ir = WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![
                IRNode { id: id_a, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
                IRNode { id: id_b, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
                IRNode { id: id_orphan, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            ],
            edges: vec![IREdge { from: id_a, to: id_b, condition: None }],
            metadata: IRMetadata { policy_applied: vec![], policy_version: 0, estimated_cost: NanoUSD::ZERO, estimated_tokens: 0 },
        };
        let result = pass.apply(ir).await.expect("pass should succeed");
        assert_eq!(result.nodes.len(), 2, "orphan node should be eliminated");
        assert!(result.nodes.iter().all(|n| n.id != id_orphan));
        assert_eq!(result.edges.len(), 1);
    }

    #[tokio::test]
    async fn test_dead_node_elimination_single_live_chain_unchanged() {
        let pass = DeadNodeEliminationPass;
        let id_a = uuid::Uuid::new_v4();
        let id_b = uuid::Uuid::new_v4();
        let ir = WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![
                IRNode { id: id_a, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
                IRNode { id: id_b, kind: IRNodeKind::Generate, strategy: StrategyKind::Single, model: None, config: HashMap::new() },
            ],
            edges: vec![IREdge { from: id_a, to: id_b, condition: None }],
            metadata: IRMetadata { policy_applied: vec![], policy_version: 0, estimated_cost: NanoUSD::ZERO, estimated_tokens: 0 },
        };
        let result = pass.apply(ir).await.expect("pass should succeed");
        assert_eq!(result.nodes.len(), 2);
        assert_eq!(result.edges.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Phase 3: build_compiler factory tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_compiler_pass_order_without_policy() {
        let rm: Arc<dyn fusion_kernel::resource::ResourceManager> = Arc::new(StubResourceManager::new(fusion_kernel::resource::Quota { max_daily_cost: NanoUSD::from_nanos(u64::MAX), max_daily_tokens: u64::MAX }));
        let engine = build_compiler(ModelCatalog::default(), rm, None);
        let names: Vec<&str> = engine.passes.iter().map(|p| p.name()).collect();
        assert_eq!(names, vec![
            "constraint_validation",
            "control_flow_validation",
            "strategy_lowering",
            "dead_node_elimination",
            "model_resolution",
            "budget_optimisation",
        ]);
    }

    #[test]
    fn test_build_compiler_appends_policy_when_provided() {
        let rm: Arc<dyn fusion_kernel::resource::ResourceManager> = Arc::new(StubResourceManager::new(fusion_kernel::resource::Quota { max_daily_cost: NanoUSD::from_nanos(u64::MAX), max_daily_tokens: u64::MAX }));
        let policy = policy_ir_with_deny("shell.exec");
        let engine = build_compiler(ModelCatalog::default(), rm, Some(policy));
        let names: Vec<&str> = engine.passes.iter().map(|p| p.name()).collect();
        assert_eq!(names.len(), 7);
        assert_eq!(names[6], "policy");
    }

    #[tokio::test]
    async fn test_build_compiler_deny_blocks_through_factory() {
        let rm: Arc<dyn fusion_kernel::resource::ResourceManager> = Arc::new(StubResourceManager::new(fusion_kernel::resource::Quota { max_daily_cost: NanoUSD::from_nanos(u64::MAX), max_daily_tokens: u64::MAX }));
        let engine = build_compiler(ModelCatalog::default(), rm, Some(policy_ir_with_deny("shell.exec")));
        let mut config = HashMap::new();
        config.insert("capability".into(), serde_json::json!("shell.exec"));
        let ir = WorkflowIR {
            plan_id: uuid::Uuid::new_v4(),
            nodes: vec![IRNode {
                id: uuid::Uuid::new_v4(),
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: None,
                config,
            }],
            edges: vec![],
            metadata: IRMetadata { policy_applied: vec![], policy_version: 0, estimated_cost: NanoUSD::from_nanos(10_000_000), estimated_tokens: 100 },
        };
        let result = engine.compile("test", &ir).await;
        assert!(result.is_err(), "deny policy should block compilation through factory");
    }

    // -----------------------------------------------------------------------
    // Phase 5: route scoring
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_default_engine_scores_not_all_tied() {
        let engine = CompilerEngine::new();
        let ir = test_ir();
        let scores: Vec<ExplainRouteScore> = vec![
            engine.explain_route("openrouter", "general question", &ir).await,
            engine.explain_route("zen", "general question", &ir).await,
            engine.explain_route("ollama", "general question", &ir).await,
        ];
        let totals: Vec<f64> = scores.iter().map(|s| s.total_score).collect();
        assert!(totals.iter().any(|t| *t < 1.0), "no provider may tie at 1.0 by default");
        assert!(
            scores.iter().all(|s| s.capability_score.is_some() && s.health_score.is_some()),
            "default engine must produce ≥1 non-budget score for every provider"
        );
    }

    #[tokio::test]
    async fn test_missing_scorer_renormalizes_weights() {
        let engine = CompilerEngine::new().with_score_sources(score::ScoreSources {
            latency: None,
            ..score::ScoreSources::default()
        });
        let score = engine.explain_route("openrouter", "general question", &test_ir()).await;
        assert_eq!(score.latency_score, None, "removed scorer must yield None");
        // cap 0.9*0.3 + bud 1.0*0.25 + hea 1.0*0.15 + pol 1.0*0.1 over 0.8 weight
        assert!((score.total_score - 0.9625).abs() < 1e-9, "total was {}", score.total_score);
    }

    #[tokio::test]
    async fn test_policy_deny_scorer_zeroes_provider_total() {
        let engine = CompilerEngine::new().with_score_sources(score::ScoreSources {
            policy: Some(Arc::new(score::StaticPolicyScorer::deny(&["openrouter"]))),
            ..score::ScoreSources::default()
        });
        let ir = test_ir();
        let denied = engine.explain_route("openrouter", "general question", &ir).await;
        let allowed = engine.explain_route("zen", "general question", &ir).await;
        assert_eq!(denied.policy_score, Some(0.0));
        assert_eq!(allowed.policy_score, Some(1.0));
        assert!(denied.total_score < allowed.total_score);
    }

    #[tokio::test]
    async fn test_report_route_scores_carry_capability() {
        let engine = CompilerEngine::new();
        let report = engine.compile("Code Generation", &test_ir()).await.expect("Compile");
        assert_eq!(report.route_scores.len(), 3);
        for score in &report.route_scores {
            assert!(score.capability_score.is_some(), "report must carry capability scores");
        }
        // "Code Generation" intent boosts openrouter capability
        let openrouter = report.route_scores.iter().find(|s| s.provider_name == "openrouter").unwrap();
        let cap = openrouter.capability_score.expect("capability score");
        assert!((cap - 0.95).abs() < 1e-9, "expected 0.95, got {cap}");
    }
}
