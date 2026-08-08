//! **SIMULATION** — Studio-sandbox compiler (v0.14 UI vertical).
//!
//! NOT wired into the production `src/` monolith compiler. Passes are no-ops and
//! provider scores in `explain_route` / `compile` are hardcoded placeholder data
//! for the Studio UI. Callers must pass `is_simulation = true` (see
//! `fusion-studio-api`).
use fusion_core::PlatformError;
use fusion_ir::WorkflowIR;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainRouteScore {
    pub provider_name: String,
    pub capability_score: f64,
    pub budget_score: f64,
    pub latency_score: f64,
    pub health_score: f64,
    pub policy_score: f64,
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
    pub ir_version: u16,
    pub passes_executed: Vec<String>,
    pub pass_diffs: Vec<CompilerPassDiff>,
    pub graph_id: String,
    pub compilation_time_ms: u64,
    pub is_simulation: bool,
    pub route_scores: Vec<ExplainRouteScore>,
    pub provider_comparison: Vec<ProviderComparisonCandidate>,
}

pub trait CompilerPass: Send + Sync {
    fn name(&self) -> &str;
    fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError>;
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
        self.engine.compile("Default Compilation", &ir, false)
    }
}

pub struct ConstraintValidationPass;
impl CompilerPass for ConstraintValidationPass {
    fn name(&self) -> &str {
        "constraint_validation"
    }

    fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        if ir.nodes.is_empty() {
            return Err(PlatformError::Compiler {
                code: "EMPTY_IR".to_string(),
                message: "IR must have at least one node".to_string(),
                recovery_suggestion: "Add at least one execution node to the workflow spec".to_string(),
            });
        }
        Ok(ir.clone())
    }
}

pub struct CapabilityResolutionPass;
impl CompilerPass for CapabilityResolutionPass {
    fn name(&self) -> &str { "Capability Resolution" }
    fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct ConstraintSolverPass;
impl CompilerPass for ConstraintSolverPass {
    fn name(&self) -> &str { "Constraint Solver" }
    fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct ConstantFoldingPass;
impl CompilerPass for ConstantFoldingPass {
    fn name(&self) -> &str { "Constant Folding" }
    fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct DeadNodeEliminationPass;
impl CompilerPass for DeadNodeEliminationPass {
    fn name(&self) -> &str { "Dead Node Elimination" }
    fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct NodeFusionPass;
impl CompilerPass for NodeFusionPass {
    fn name(&self) -> &str { "Node Fusion" }
    fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct RetryInjectionPass;
impl CompilerPass for RetryInjectionPass {
    fn name(&self) -> &str { "Retry Injection" }
    fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct FallbackInjectionPass;
impl CompilerPass for FallbackInjectionPass {
    fn name(&self) -> &str { "Fallback Injection" }
    fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct SchedulingHintsPass;
impl CompilerPass for SchedulingHintsPass {
    fn name(&self) -> &str { "Scheduling Hints" }
    fn transform(&self, ir: &WorkflowIR) -> Result<WorkflowIR, PlatformError> {
        Ok(ir.clone())
    }
}

pub struct CompilerEngine {
    passes: Vec<Box<dyn CompilerPass>>,
}

impl CompilerEngine {
    pub fn new() -> Self {
        let passes: Vec<Box<dyn CompilerPass>> = vec![
            Box::new(ValidationPass),
            Box::new(CapabilityResolutionPass),
            Box::new(ConstraintSolverPass),
            Box::new(ConstantFoldingPass),
            Box::new(DeadNodeEliminationPass),
            Box::new(NodeFusionPass),
            Box::new(RetryInjectionPass),
            Box::new(FallbackInjectionPass),
            Box::new(SchedulingHintsPass),
        ];
        Self { passes }
    }

    pub fn compile(&self, intent: &str, ir: &WorkflowIR, is_simulation: bool) -> Result<CompilerReport, PlatformError> {
        if intent.is_empty() {
            return Err(PlatformError::Compiler {
                code: "EMPTY_INTENT".to_string(),
                message: "Compiler intent cannot be empty".to_string(),
                recovery_suggestion: "Provide valid intent string".to_string(),
            });
        }

        let mut current_ir = ir.clone();
        let mut pass_names = Vec::new();
        let mut pass_diffs = Vec::new();

        for (idx, pass) in self.passes.iter().enumerate() {
            let pass_name = pass.name().to_string();
            let input_count = current_ir.nodes().len();
            current_ir = pass.transform(&current_ir)?;
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

        let route_scores = vec![
            self.explain_route("openrouter"),
            self.explain_route("zen"),
            self.explain_route("ollama"),
        ];

        let provider_comparison = vec![
            ProviderComparisonCandidate { provider_name: "openrouter".to_string(), model_name: "claude-3-5-sonnet".to_string(), total_score: 0.97, status: "Selected".to_string(), reason: "Highest weighted score across capability & health".to_string() },
            ProviderComparisonCandidate { provider_name: "zen".to_string(), model_name: "gemini-2.5-pro".to_string(), total_score: 0.93, status: "Alternative".to_string(), reason: "Slightly higher latency (42ms vs 38ms)".to_string() },
            ProviderComparisonCandidate { provider_name: "ollama".to_string(), model_name: "llama3".to_string(), total_score: 0.78, status: "Filtered".to_string(), reason: "Missing vision capability for requested prompt".to_string() },
        ];

        Ok(CompilerReport {
            intent: intent.to_string(),
            ir_version: ir.version(),
            passes_executed: pass_names,
            pass_diffs,
            graph_id: format!("graph_{}", ir.workflow_id()),
            compilation_time_ms: 2,
            is_simulation,
            route_scores,
            provider_comparison,
        })
    }

    pub fn explain_route(&self, provider_name: &str) -> ExplainRouteScore {
        let (cap, bud, lat, hea, pol) = match provider_name {
            "openrouter" => (0.95, 0.90, 0.85, 1.00, 1.00),
            "zen" => (0.90, 0.95, 0.90, 0.98, 1.00),
            "ollama" => (0.80, 1.00, 0.95, 0.90, 1.00),
            _ => (0.50, 0.50, 0.50, 0.50, 1.00),
        };
        let total = (cap * 0.3) + (bud * 0.25) + (lat * 0.2) + (hea * 0.15) + (pol * 0.1);

        ExplainRouteScore {
            provider_name: provider_name.to_string(),
            capability_score: cap,
            budget_score: bud,
            latency_score: lat,
            health_score: hea,
            policy_score: pol,
            total_score: total,
        }
    }
}

impl Default for CompilerEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiler_engine_pass_pipeline() {
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

        let report = engine.compile("Code Generation", &ir, false).expect("Compile");
        assert_eq!(report.passes_executed.len(), 9);
        assert_eq!(report.pass_diffs.len(), 9);
    }

    #[test]
    fn test_constraint_validation_pass() {
        let pass = ConstraintValidationPass;
        let empty_ir = WorkflowIR { nodes: vec![], edges: vec![] };
        let res = pass.transform(&empty_ir);
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
        assert!(pass.transform(&valid_ir).is_ok());
    }
}
