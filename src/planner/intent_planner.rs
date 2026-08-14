use async_trait::async_trait;

use super::Planner;
use crate::providers::capability_catalog::CapabilityCatalog;
use crate::types::execution::ExecutionIntent;
use crate::types::{
    ComplexityLevel, EvidenceSnapshot, IRNodeKind, Intent, ModelCatalog, NanoUSD, Policy, Requirements,
    StrategyKind, WorkflowIR,
};

pub struct IntentPlanner {
    pub model_catalog: ModelCatalog,
    pub capability_catalog: Option<CapabilityCatalog>,
}

impl IntentPlanner {
    pub fn new(model_catalog: ModelCatalog) -> Self {
        Self {
            model_catalog,
            capability_catalog: None,
        }
    }

    pub fn with_capability_catalog(
        model_catalog: ModelCatalog,
        catalog: CapabilityCatalog,
    ) -> Self {
        Self {
            model_catalog,
            capability_catalog: Some(catalog),
        }
    }

    fn select_model(&self, requirements: &Requirements) -> String {
        let base_model = match requirements.intent_classification {
            Intent::Code | Intent::Debug => self.model_catalog.code.clone(),
            Intent::Architecture => self.model_catalog.architecture.clone(),
            Intent::Analysis => self.model_catalog.analysis.clone(),
            Intent::Creative => self.model_catalog.creative.clone(),
            Intent::General => self.model_catalog.general.clone(),
        };

        if let Some(ref catalog) = self.capability_catalog {
            let model_reqs = requirements.model_requirements.clone().unwrap_or_default();
            let candidates = catalog.resolve(&model_reqs);
            if let Some(best) = candidates.first() {
                return format!("{}/{}", best.provider_name, best.model_id);
            }
        }

        base_model
    }

    fn resolve_model_for_step(
        &self,
        step: &str,
        intent: &ExecutionIntent,
        fallback: &str,
    ) -> String {
        let catalog = match &self.capability_catalog {
            Some(cat) => cat,
            None => return fallback.to_string(),
        };

        let candidates = catalog.query_by_capability(step);
        if candidates.is_empty() {
            return fallback.to_string();
        }

        let valid_candidates: Vec<_> = candidates
            .iter()
            .filter(|c| {
                c.capabilities.reasoning_score.is_finite()
                    && c.capabilities.coding_score.is_finite()
                    && c.pricing.input_cost_per_1k.is_finite()
            })
            .collect();

        if valid_candidates.is_empty() {
            return fallback.to_string();
        }

        let best = match intent {
            ExecutionIntent::Quality | ExecutionIntent::Exhaustive => {
                valid_candidates.iter().copied().max_by(|a, b| {
                    a.capabilities
                        .reasoning_score
                        .partial_cmp(&b.capabilities.reasoning_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            }
            ExecutionIntent::Speed => {
                valid_candidates.iter().copied().min_by(|a, b| {
                    a.pricing
                        .input_cost_per_1k
                        .partial_cmp(&b.pricing.input_cost_per_1k)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            }
            ExecutionIntent::Balanced => {
                valid_candidates.iter().copied().max_by(|a, b| {
                    let score_a =
                        a.capabilities.reasoning_score * 0.5 + a.capabilities.coding_score * 0.5;
                    let score_b =
                        b.capabilities.reasoning_score * 0.5 + b.capabilities.coding_score * 0.5;
                    score_a
                        .partial_cmp(&score_b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            }
            ExecutionIntent::Constrained { .. } => {
                valid_candidates.iter().copied().max_by(|a, b| {
                    let score_a =
                        a.capabilities.reasoning_score * 0.5 + a.capabilities.coding_score * 0.5;
                    let score_b =
                        b.capabilities.reasoning_score * 0.5 + b.capabilities.coding_score * 0.5;
                    score_a
                        .partial_cmp(&score_b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            }
        };

        best.map(|c| format!("{}/{}", c.provider_name, c.model_id))
            .unwrap_or_else(|| fallback.to_string())
    }

    fn to_fusion_core_catalog(catalog: &ModelCatalog) -> fusion_core::ModelCatalog {
        fusion_core::ModelCatalog {
            code: catalog.code.clone(),
            debug: catalog.debug.clone(),
            architecture: catalog.architecture.clone(),
            general: catalog.general.clone(),
            creative: catalog.creative.clone(),
            analysis: catalog.analysis.clone(),
            fast: catalog.fast.clone(),
            cheap: catalog.cheap.clone(),
        }
    }

    fn plan_from_crates(
        &self,
        intent: &ExecutionIntent,
        model: &str,
        requirements: &Requirements,
        policies: &[Policy],
    ) -> Result<WorkflowIR, String> {
        let crates_intent = match intent {
            ExecutionIntent::Quality => fusion_planner::ExecutionIntent::Quality,
            ExecutionIntent::Speed => fusion_planner::ExecutionIntent::Speed,
            ExecutionIntent::Balanced => fusion_planner::ExecutionIntent::Balanced,
            ExecutionIntent::Exhaustive => fusion_planner::ExecutionIntent::Exhaustive,
            ExecutionIntent::Constrained { max_cost, .. } => {
                fusion_planner::ExecutionIntent::Constrained { max_cost: *max_cost }
            }
        };

        let mut policy_declarations = Vec::new();
        for p in policies {
            policy_declarations.push(fusion_planner::PolicyDeclarationSnapshot {
                id: p.name.clone(),
                name: p.name.clone(),
                rule: format!("{:?}", p.actions),
            });
        }

        let req = fusion_planner::PlanningRequest {
            intent: crates_intent,
            user_prompt: requirements.original_text.clone(),
            requested_model: Some(model.to_string()),
            requested_strategy: None,
            strategy_config: None,
            requirements: fusion_planner::RequirementsSnapshot {
                complexity: format!("{:?}", requirements.complexity),
                execution_intent: None,
                required_capabilities: vec![],
            },
            policies: fusion_planner::PolicySnapshot {
                version: 1,
                policies: policy_declarations,
                created_at: 0,
            },
            capability_catalog: fusion_planner::CapabilityCatalogSnapshot::new(fusion_kernel::CapabilityCatalog::new()),
            model_catalog: fusion_planner::ModelCatalogSnapshot::new(Self::to_fusion_core_catalog(&self.model_catalog)),
            telemetry: fusion_planner::RoutingTelemetrySnapshot::default(),
        };
        let planner = fusion_planner::IntentPlanner::new(Self::to_fusion_core_catalog(&self.model_catalog));
        let contract = match planner.plan(&req) {
            Ok(c) => c,
            Err(e) => return Err(format!("fusion_planner error: {:?}", e)),
        };
        crate::ir::adapter::workflow_to_types(&contract).map_err(|e| format!("adapter error: {}", e))
    }
}

#[async_trait]
impl Planner for IntentPlanner {
    async fn plan(
        &self,
        requirements: &Requirements,
        policies: &[Policy],
        _evidence: Option<&EvidenceSnapshot>,
    ) -> WorkflowIR {
        let model = self.select_model(requirements);

        let intent = requirements.execution_intent.clone().unwrap_or_else(|| {
            match requirements.complexity {
                ComplexityLevel::Critical => ExecutionIntent::Quality,
                ComplexityLevel::High => ExecutionIntent::Balanced,
                ComplexityLevel::Medium | ComplexityLevel::Low => ExecutionIntent::Speed,
            }
        });
        self.plan_from_crates(&intent, &model, requirements, policies)
            .unwrap_or_else(|e| panic!("Planning failure in fusion-planner for intent {:?}: {}", intent, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn make_reqs(execution_intent: Option<ExecutionIntent>) -> Requirements {
        Requirements {
            intent_classification: Intent::General,
            complexity: ComplexityLevel::Medium,
            has_files: false,
            context_window: 4096,
            original_text: "test".to_string(),
            execution_intent,
            output_preferences: None,
            model_requirements: None,
            requested_strategy: None,
        }
    }

    fn make_planner() -> IntentPlanner {
        IntentPlanner::new(ModelCatalog {
            code: "test-code-model".into(),
            debug: "test-debug-model".into(),
            architecture: "test-arch-model".into(),
            general: "test-general-model".into(),
            creative: "test-creative-model".into(),
            analysis: "test-analysis-model".into(),
            fast: "test-fast-model".into(),
            cheap: "test-cheap-model".into(),
        })
    }

    #[tokio::test]
    async fn test_quality_plan_node_count() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Quality));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 5);
        assert_eq!(ir.metadata.estimated_cost, NanoUSD::from_nanos(50_000_000));
        assert_eq!(ir.metadata.estimated_tokens, 5000);
        assert!(ir
            .metadata
            .policy_applied
            .contains(&"intent:quality".to_string()));
    }

    #[tokio::test]
    async fn test_quality_plan_node_kinds() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Quality));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 5);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[3].kind, IRNodeKind::Judge);
        assert_eq!(ir.nodes[4].strategy, StrategyKind::Reflection);
    }

    #[tokio::test]
    async fn test_speed_plan_single_node() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Speed));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 1);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[0].strategy, StrategyKind::Single);
        assert!(ir
            .metadata
            .policy_applied
            .contains(&"intent:speed".to_string()));
        assert_eq!(ir.metadata.estimated_cost, NanoUSD::from_nanos(10_000_000));
        assert_eq!(ir.metadata.estimated_tokens, 1000);
    }

    #[tokio::test]
    async fn test_balanced_plan_three_nodes() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Balanced));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 3);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[1].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[2].kind, IRNodeKind::Judge);
        assert!(ir
            .metadata
            .policy_applied
            .contains(&"intent:balanced".to_string()));
        assert_eq!(ir.metadata.estimated_cost, NanoUSD::from_nanos(30_000_000));
    }

    #[tokio::test]
    async fn test_exhaustive_plan_six_nodes() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Exhaustive));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 6);
        assert!(ir
            .metadata
            .policy_applied
            .contains(&"intent:exhaustive".to_string()));
        assert_eq!(ir.metadata.estimated_cost, NanoUSD::from_nanos(80_000_000));
        assert_eq!(ir.metadata.estimated_tokens, 8000);
    }

    #[tokio::test]
    async fn test_exhaustive_plan_ends_with_consensus_judge() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Exhaustive));
        let ir = planner.plan(&reqs, &[], None).await;
        let last = ir.nodes.last().unwrap();
        assert_eq!(last.kind, IRNodeKind::Judge);
        assert_eq!(last.strategy, StrategyKind::Consensus);
    }

    #[tokio::test]
    async fn test_constrained_cheap_returns_speed() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Constrained {
            max_latency_ms: None,
            max_cost: Some(NanoUSD::from_nanos(10_000_000)),
            max_tokens: None,
            min_confidence: None,
        }));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 1);
    }

    #[tokio::test]
    async fn test_constrained_generous_budget_returns_balanced() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Constrained {
            max_latency_ms: None,
            max_cost: Some(NanoUSD::from_nanos(50_000_000)),
            max_tokens: None,
            min_confidence: None,
        }));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 3);
    }

    #[tokio::test]
    async fn test_constrained_no_budget_returns_balanced() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Constrained {
            max_latency_ms: None,
            max_cost: None,
            max_tokens: None,
            min_confidence: None,
        }));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 3);
    }

    #[tokio::test]
    async fn test_constrained_exact_at_threshold_returns_balanced() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Constrained {
            max_latency_ms: None,
            max_cost: Some(NanoUSD::from_nanos(20_000_000)),
            max_tokens: None,
            min_confidence: None,
        }));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 3);
    }

    #[tokio::test]
    async fn test_no_intent_critical_complexity_returns_quality() {
        let planner = make_planner();
        let mut reqs = make_reqs(None);
        reqs.complexity = ComplexityLevel::Critical;
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 5);
    }

    #[tokio::test]
    async fn test_no_intent_high_complexity_returns_balanced() {
        let planner = make_planner();
        let mut reqs = make_reqs(None);
        reqs.complexity = ComplexityLevel::High;
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 3);
    }

    #[tokio::test]
    async fn test_no_intent_medium_complexity_returns_speed() {
        let planner = make_planner();
        let mut reqs = make_reqs(None);
        reqs.complexity = ComplexityLevel::Medium;
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 1);
    }

    #[tokio::test]
    async fn test_no_intent_low_complexity_returns_speed() {
        let planner = make_planner();
        let mut reqs = make_reqs(None);
        reqs.complexity = ComplexityLevel::Low;
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 1);
    }

    #[tokio::test]
    async fn test_select_model_returns_non_empty_string() {
        let planner = make_planner();
        let model = planner.select_model(&make_reqs(None));
        assert!(!model.is_empty());
    }

    #[tokio::test]
    async fn test_each_intent_produces_distinct_plan_ids() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Quality));
        let ir1 = planner.plan(&reqs, &[], None).await;
        let reqs = make_reqs(Some(ExecutionIntent::Speed));
        let ir2 = planner.plan(&reqs, &[], None).await;
        assert_ne!(ir1.plan_id, ir2.plan_id);
    }

    #[tokio::test]
    async fn test_plan_nodes_have_unique_ids() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Exhaustive));
        let ir = planner.plan(&reqs, &[], None).await;
        let mut ids: Vec<_> = ir.nodes.iter().map(|n| n.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ir.nodes.len());
    }

    #[tokio::test]
    async fn test_speed_plan_has_no_edges() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Speed));
        let ir = planner.plan(&reqs, &[], None).await;
        assert!(
            ir.edges.is_empty(),
            "Speed plan should have no edges (single node)"
        );
    }

    #[tokio::test]
    async fn test_quality_plan_has_edges() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Quality));
        let ir = planner.plan(&reqs, &[], None).await;
        assert!(!ir.edges.is_empty(), "Quality plan should have edges");
        assert_eq!(ir.edges.len(), 4, "Quality plan should have 4 edges");
    }

    #[tokio::test]
    async fn test_balanced_plan_has_edges() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Balanced));
        let ir = planner.plan(&reqs, &[], None).await;
        assert!(!ir.edges.is_empty(), "Balanced plan should have edges");
        assert_eq!(ir.edges.len(), 2, "Balanced plan should have 2 edges");
    }

    #[tokio::test]
    async fn test_exhaustive_plan_has_edges() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Exhaustive));
        let ir = planner.plan(&reqs, &[], None).await;
        assert!(!ir.edges.is_empty(), "Exhaustive plan should have edges");
        assert_eq!(ir.edges.len(), 5, "Exhaustive plan should have 5 edges");
    }

    #[tokio::test]
    async fn test_edges_reference_valid_node_ids() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Quality));
        let ir = planner.plan(&reqs, &[], None).await;
        let node_ids: std::collections::HashSet<_> = ir.nodes.iter().map(|n| n.id).collect();
        for edge in &ir.edges {
            assert!(
                node_ids.contains(&edge.from),
                "Edge from {:?} references invalid node",
                edge.from
            );
            assert!(
                node_ids.contains(&edge.to),
                "Edge to {:?} references invalid node",
                edge.to
            );
        }
    }
}
