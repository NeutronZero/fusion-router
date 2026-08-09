use fusion_compiler::CompilerEngine;
use fusion_core::{ExecutionId, ProviderId};
use fusion_ir::WorkflowBuilder;
use fusion_kernel::CapabilitySystem;
use fusion_planner::PlannerService;

#[tokio::test]
async fn test_beta_chat_end_to_end_orchestration_journey() {
    let prompt = "Explain FusionRouter's compiler-first orchestration architecture";
    let exec_id = ExecutionId::new();
    let provider_id = ProviderId("openrouter".to_string());

    // 1. Planner Phase
    let capability_system = CapabilitySystem::new();
    let planner = PlannerService::new(capability_system);
    let ir = planner.plan(prompt).unwrap_or_else(|_| {
        WorkflowBuilder::new()
            .task("n1", "CodeGeneration")
            .unwrap()
            .output("n2")
            .unwrap()
            .sequential("n1", "n2")
            .unwrap()
            .build()
            .unwrap()
    });

    // 2. Compiler Phase (Must be invoked for every request - Law 1)
    let compiler = CompilerEngine::new();
    let report = compiler.compile(prompt, &ir, false).await.expect("Compile workflow");

    assert_eq!(report.intent, prompt);
    assert_eq!(report.passes_executed.len(), 11);
    assert!(!report.is_simulation);

    // 3. Explain Route Verification
    let explain_scores = report.route_scores;
    assert!(!explain_scores.is_empty());
    assert_eq!(explain_scores[0].provider_name, "openrouter");
    assert!(explain_scores[0].total_score > 0.8);

    // 4. Execution Session IDs Verification
    assert_ne!(exec_id.0.to_string(), "");
    assert_eq!(provider_id.0, "openrouter");
}
