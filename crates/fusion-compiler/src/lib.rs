//! Compiler pass pipeline for WorkflowIR optimisation.
use std::sync::Arc;
use fusion_core::{ModelCatalog, ModelRequirements, PlatformError};
use fusion_ir::WorkflowIR;
use fusion_kernel::resource::StubResourceManager;
use passes::BudgetOptimisationPass;
use serde::{Deserialize, Serialize};

pub mod passes;

/// Weights for sub-scores in the route scoring formula.
/// Used to compute `total_score` from available `Option<f64>` sub-scores.
const WEIGHT_CAPABILITY: f64 = 0.3;
const WEIGHT_BUDGET: f64 = 0.25;
const WEIGHT_LATENCY: f64 = 0.2;
const WEIGHT_HEALTH: f64 = 0.15;
const WEIGHT_POLICY: f64 = 0.1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainRouteScore {
    pub provider_name: String,
    /// Capability resolution score. `None` = not yet wired in crates/
    /// (real resolution lives in frozen `src/planner/resolver/`).
    pub capability_score: Option<f64>,
    /// Budget affordability score. `Some(1.0)` from StubResourceManager
    /// (always permissive per ADR-038's deliberate scope).
    pub budget_score: Option<f64>,
    /// Latency score from health checker. `None` = no live data in crates/
    /// (ConnectorHealthChecker in `src/scheduler/` is unpopulated for crate path).
    pub latency_score: Option<f64>,
    /// Health score from health checker. `None` = no live data in crates/.
    pub health_score: Option<f64>,
    /// Policy compliance score. `None` = not wired in crates/
    /// (real policy logic lives in frozen `src/compiler/passes/policy.rs`).
    pub policy_score: Option<f64>,
    /// Weighted total of available sub-scores. Computed from `Some(...)` values
    /// only; missing scores are excluded and weights re-normalized.
    /// `0.0` if all sub-scores are `None`.
    pub total_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderComparisonCandidate {
    pub provider_name: String,
    pub model_name: String,
    pub total_score: f64,
    pub status: String,
    /// Dynamically generated reason based on score deltas.
    /// `"Score not computed"` when sub-scores are `None`.
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
    pub ir_version: u16,
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
    async fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError>;
}

#[async_trait::async_trait]
pub trait Compiler: Send + Sync {
    async fn compile(&self, ir: WorkflowIR) -> Result<CompilerReport, PlatformError>;
}

pub struct DefaultCompiler {
    engine: CompilerEngine,
}

impl DefaultCompiler {
    pub fn new() -> Self {
        Self {
            engine: CompilerEngine::new(),
        }
    }
}

impl Default for DefaultCompiler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Compiler for DefaultCompiler {
    async fn compile(&self, ir: WorkflowIR) -> Result<CompilerReport, PlatformError> {
        self.engine.compile("Default Compilation", &ir).await
    }
}

pub struct ConstraintValidationPass;
#[async_trait::async_trait]
impl CompilerPass for ConstraintValidationPass {
    fn name(&self) -> &str {
        "constraint_validation"
    }

    async fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        if ir.nodes().is_empty() {
            return Err(PlatformError::Compiler {
                code: "EMPTY_IR".to_string(),
                message: "IR must have at least one node".to_string(),
                recovery_suggestion: "Add at least one execution node to the workflow spec".to_string(),
            });
        }
        Ok(ir.clone())
    }
}

pub struct ModelResolutionPass {
    pub model_catalog: ModelCatalog,
    pub model_requirements: Option<ModelRequirements>,
}

impl ModelResolutionPass {
    pub fn new(model_catalog: ModelCatalog, model_requirements: Option<ModelRequirements>) -> Self {
        Self {
            model_catalog,
            model_requirements,
        }
    }

    pub fn select_model(&self) -> &str {
        match &self.model_requirements {
            Some(reqs) if reqs.requires_tools => &self.model_catalog.code,
            Some(reqs) if reqs.min_coding_score.is_some_and(|s| s >= 0.8) => &self.model_catalog.code,
            Some(reqs) if reqs.min_reasoning_score.is_some_and(|s| s >= 0.8) => &self.model_catalog.architecture,
            _ => &self.model_catalog.fast,
        }
    }
}

#[async_trait::async_trait]
impl CompilerPass for ModelResolutionPass {
    fn name(&self) -> &str {
        "model_resolution"
    }

    async fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct ControlFlowValidationPass;

#[async_trait::async_trait]
impl CompilerPass for ControlFlowValidationPass {
    fn name(&self) -> &str {
        "control_flow_validation"
    }

    async fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        let report = ir.validate();
        if let Some(err) = report.first_error() {
            return Err(PlatformError::Compiler {
                code: "CONTROL_FLOW_VALIDATION_FAILED".to_string(),
                message: format!("Control flow validation error: {err}"),
                recovery_suggestion: "Ensure edge integrity, cycle constraints, and node arity requirements are satisfied".to_string(),
            });
        }
        Ok(ir.clone())
    }
}

pub struct CapabilityResolutionPass;
#[async_trait::async_trait]
impl CompilerPass for CapabilityResolutionPass {
    fn name(&self) -> &str { "Capability Resolution" }
    async fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct ConstraintSolverPass;
#[async_trait::async_trait]
impl CompilerPass for ConstraintSolverPass {
    fn name(&self) -> &str { "Constraint Solver" }
    async fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct ConstantFoldingPass;
#[async_trait::async_trait]
impl CompilerPass for ConstantFoldingPass {
    fn name(&self) -> &str { "Constant Folding" }
    async fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct DeadNodeEliminationPass;
#[async_trait::async_trait]
impl CompilerPass for DeadNodeEliminationPass {
    fn name(&self) -> &str { "Dead Node Elimination" }
    async fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct NodeFusionPass;
#[async_trait::async_trait]
impl CompilerPass for NodeFusionPass {
    fn name(&self) -> &str { "Node Fusion" }
    async fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct RetryInjectionPass;
#[async_trait::async_trait]
impl CompilerPass for RetryInjectionPass {
    fn name(&self) -> &str { "Retry Injection" }
    async fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct FallbackInjectionPass;
#[async_trait::async_trait]
impl CompilerPass for FallbackInjectionPass {
    fn name(&self) -> &str { "Fallback Injection" }
    async fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct SchedulingHintsPass;
#[async_trait::async_trait]
impl CompilerPass for SchedulingHintsPass {
    fn name(&self) -> &str { "Scheduling Hints" }
    async fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct CompilerEngine {
    passes: Vec<Box<dyn CompilerPass>>,
}

impl CompilerEngine {
    pub fn new() -> Self {
        let passes: Vec<Box<dyn CompilerPass>> = vec![
            Box::new(ConstraintValidationPass),
            Box::new(ControlFlowValidationPass),
            Box::new(CapabilityResolutionPass),
            Box::new(ConstraintSolverPass),
            Box::new(ConstantFoldingPass),
            Box::new(DeadNodeEliminationPass),
            Box::new(NodeFusionPass),
            Box::new(RetryInjectionPass),
            Box::new(FallbackInjectionPass),
            Box::new(SchedulingHintsPass),
            Box::new(BudgetOptimisationPass::new(Arc::new(StubResourceManager::new(f64::INFINITY, u64::MAX)))),
        ];
        Self { passes }
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
            let input_count = current_ir.nodes().len();
            current_ir = pass.transform(&current_ir).await?;
            let output_count = current_ir.nodes().len();

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

        // Temporary fallback: fixed provider list until capability resolution
        // is wired into crates/ (see ADR-039 D2).
        let route_scores = vec![
            self.explain_route("openrouter"),
            self.explain_route("zen"),
            self.explain_route("ollama"),
        ];

        let provider_comparison = Self::build_provider_comparison(&route_scores);

        Ok(CompilerReport {
            intent: intent.to_string(),
            ir_version: ir.version(),
            passes_executed: pass_names,
            pass_diffs,
            graph_id: format!("graph_{}", ir.workflow_id()),
            compilation_time_ms,
            route_scores,
            provider_comparison,
        })
    }

    /// Scores a provider against the given IR.
    ///
    /// Sub-scores are `Option<f64>` — `None` means "not yet wired in crates/"
    /// (see ADR-039 for per-sub-score data source investigation).
    pub fn explain_route(&self, provider_name: &str) -> ExplainRouteScore {
        // capability_score: None — CapabilityResolutionPass in crates/ is a no-op.
        // Real resolution lives in frozen src/planner/resolver/.
        let capability_score: Option<f64> = None;

        // budget_score: Some(1.0) — StubResourceManager always permissive (ADR-038).
        let budget_score: Option<f64> = Some(1.0);

        // latency_score: None — ConnectorHealthChecker exists in src/scheduler/
        // but is unpopulated for the crate compiler path.
        let latency_score: Option<f64> = None;

        // health_score: None — same reason as latency_score.
        let health_score: Option<f64> = None;

        // policy_score: None — PolicyCompilerPass is in frozen src/compiler/.
        // Porting policy scoring to crates/ is a separate task (ADR-039).
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

    /// Builds provider comparison from computed route scores, generating
    /// reasons dynamically from score deltas instead of hardcoded prose.
    fn build_provider_comparison(scores: &[ExplainRouteScore]) -> Vec<ProviderComparisonCandidate> {
        if scores.is_empty() {
            return Vec::new();
        }

        let mut ranked: Vec<&ExplainRouteScore> = scores.iter().collect();
        ranked.sort_by(|a, b| b.total_score.partial_cmp(&a.total_score).unwrap_or(std::cmp::Ordering::Equal));

        let best_score = ranked[0].total_score;

        // Count how many providers share the top score
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
                "Score not computed — all sub-scores are None (see ADR-039)".to_string()
            } else if i == 0 && unique_best {
                format!("Highest total score ({:.2}) among evaluated providers", score.total_score)
            } else if i == 0 && !unique_best {
                format!(
                    "Tied with {} other provider(s) at {:.2} — selected by position, not a computed decision (see ADR-039 D2)",
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

            // Use first available model name based on provider
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

/// Computes total_score from available sub-scores, re-normalizing weights.
/// Missing (`None`) scores are excluded; weights are re-normalized proportionally.
/// Returns `0.0` if all sub-scores are `None`.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_compiler_engine_pass_pipeline() {
        let engine = CompilerEngine::new();
        let ir = fusion_ir::WorkflowBuilder::new()
            .task("n1", "CodeGeneration")
            .unwrap()
            .output("n2")
            .unwrap()
            .sequential("n1", "n2")
            .unwrap()
            .build()
            .unwrap();

        let report = engine.compile("Code Generation", &ir).await.expect("Compile");
        assert_eq!(report.passes_executed.len(), 11);
        assert_eq!(report.pass_diffs.len(), 11);
    }

    #[tokio::test]
    async fn test_constraint_validation_pass() {
        let pass = ConstraintValidationPass;
        // Construct an empty IR via serde directly (bypasses from_json validation)
        // to test that ConstraintValidationPass catches empty IRs itself.
        let empty_ir: fusion_ir::WorkflowIR = serde_json::from_str(&serde_json::json!({
            "version": 1,
            "workflow_id": "00000000-0000-0000-0000-000000000000",
            "nodes": [],
            "edges": [],
            "metadata": { "policy_applied": [], "estimated_cost": 0.0, "estimated_tokens": 0 }
        }).to_string()).unwrap();
        let res = pass.transform(&empty_ir).await;
        assert!(res.is_err());
        if let Err(PlatformError::Compiler { code, .. }) = res {
            assert_eq!(code, "EMPTY_IR");
        } else {
            panic!("Expected PlatformError::Compiler");
        }

        let valid_ir = fusion_ir::WorkflowBuilder::new()
            .task("n1", "CodeGeneration")
            .unwrap()
            .build()
            .unwrap();
        assert!(pass.transform(&valid_ir).await.is_ok());
    }

    #[test]
    fn test_explain_route_budget_score_is_some() {
        let engine = CompilerEngine::new();
        let score = engine.explain_route("openrouter");
        assert_eq!(score.provider_name, "openrouter");
        assert_eq!(score.budget_score, Some(1.0));
        assert_eq!(score.capability_score, None);
        assert_eq!(score.latency_score, None);
        assert_eq!(score.health_score, None);
        assert_eq!(score.policy_score, None);
    }

    #[test]
    fn test_total_score_computed_from_available_scores() {
        let engine = CompilerEngine::new();
        let score = engine.explain_route("openrouter");
        // Only budget_score is Some(1.0), weight = 0.25
        // After renormalization: total = 1.0 * (0.25 / 0.25) = 1.0
        assert_eq!(score.total_score, 1.0);
    }

    #[test]
    fn test_total_score_zero_when_all_none() {
        let total = compute_total_score(None, 0.3, None, 0.25, None, 0.2, None, 0.15, None, 0.1);
        assert_eq!(total, 0.0);
    }

    #[test]
    fn test_total_score_renormalizes_weights() {
        // Two scores: 0.8 with weight 0.3, 0.6 with weight 0.2
        // Total weight = 0.5, weighted_sum = 0.8*0.3 + 0.6*0.2 = 0.36
        // total = 0.36 / 0.5 = 0.72
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
        // All scores are 1.0 (tied) — first is "Alternative" not "Selected"
        // because no provider is uniquely best (see ADR-039 D2)
        assert_eq!(comparison[0].status, "Alternative");
        assert!(comparison[0].reason.contains("Tied with 2 other provider(s) at 1.00"));
    }

    #[test]
    fn test_provider_comparison_alternative_status() {
        // Build scores with different totals to test Alternative status
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
        assert_eq!(comparison[1].status, "Alternative"); // 0.95 >= 1.0 * 0.9
        assert_eq!(comparison[2].status, "Filtered");   // 0.5 < 1.0 * 0.9
    }

    #[test]
    fn test_provider_comparison_all_zero_scores() {
        let scores = vec![
            ExplainRouteScore {
                provider_name: "x".into(),
                capability_score: None,
                budget_score: None,
                latency_score: None,
                health_score: None,
                policy_score: None,
                total_score: 0.0,
            },
        ];
        let comparison = CompilerEngine::build_provider_comparison(&scores);
        assert_eq!(comparison[0].reason, "Score not computed — all sub-scores are None (see ADR-039)");
    }

    #[test]
    fn test_compilation_time_is_measured() {
        let engine = CompilerEngine::new();
        let ir = fusion_ir::WorkflowBuilder::new()
            .task("n1", "CodeGeneration")
            .unwrap()
            .build()
            .unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let report = rt.block_on(engine.compile("test", &ir)).unwrap();
        // compilation_time_ms should be a real measured value, not the old hardcoded 2
        // It could be 0 or more depending on system speed
        assert!(report.compilation_time_ms <= 100, "compilation_time_ms should be reasonable: {}", report.compilation_time_ms);
    }
}
