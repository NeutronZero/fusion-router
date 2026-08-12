use std::collections::HashMap;

use async_trait::async_trait;
use uuid::Uuid;

use super::Planner;
use crate::providers::capability_catalog::CapabilityCatalog;
use crate::types::execution::ExecutionIntent;
use crate::types::{
    ComplexityLevel, EvidenceSnapshot, IREdge, IRMetadata, IRNode, IRNodeKind, Intent,
    ModelCatalog, Policy, Requirements, StrategyKind, WorkflowIR,
};

pub struct IntentPlanner {
    pub model_catalog: ModelCatalog,
    pub capability_catalog: Option<CapabilityCatalog>,
}

impl IntentPlanner {
    pub fn new(model_catalog: ModelCatalog) -> Self {
        Self { model_catalog, capability_catalog: None }
    }

    pub fn with_capability_catalog(model_catalog: ModelCatalog, catalog: CapabilityCatalog) -> Self {
        Self { model_catalog, capability_catalog: Some(catalog) }
    }

    fn build_quality(&self, model: &str) -> WorkflowIR {
        let plan_id = Uuid::new_v4();
        let intent = ExecutionIntent::Quality;

        // Resolve models per step from catalog
        let gen_model = self.resolve_model_for_step("generate", &intent, model);
        let judge_model = self.resolve_model_for_step("judge", &intent, model);

        let gen1_id = Uuid::new_v4();
        let gen2_id = Uuid::new_v4();
        let gen3_id = Uuid::new_v4();
        let judge_id = Uuid::new_v4();
        let reflect_id = Uuid::new_v4();

        let nodes = vec![
            IRNode {
                id: gen1_id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(gen_model.clone()),
                config: HashMap::new(),
            },
            IRNode {
                id: gen2_id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(gen_model.clone()),
                config: HashMap::new(),
            },
            IRNode {
                id: gen3_id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(gen_model),
                config: HashMap::new(),
            },
            IRNode {
                id: judge_id,
                kind: IRNodeKind::Judge,
                strategy: StrategyKind::Single,
                model: Some(judge_model),
                config: HashMap::new(),
            },
            IRNode {
                id: reflect_id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Reflection,
                model: Some(self.resolve_model_for_step("review", &intent, model)),
                config: HashMap::new(),
            },
        ];

        // Chain: gen1 -> gen2 -> gen3 -> judge -> reflect
        let edges = vec![
            IREdge { from: gen1_id, to: gen2_id, condition: None },
            IREdge { from: gen2_id, to: gen3_id, condition: None },
            IREdge { from: gen3_id, to: judge_id, condition: None },
            IREdge { from: judge_id, to: reflect_id, condition: None },
        ];

        let plan = WorkflowIR {
            plan_id,
            nodes,
            edges,
            metadata: IRMetadata {
                policy_applied: vec!["intent:quality".into()],
                estimated_cost: 0.05,
                estimated_tokens: 5000,
            },
        };

        #[cfg(debug_assertions)]
        Self::validate_plan(&plan);

        plan
    }

    fn build_speed(&self, model: &str) -> WorkflowIR {
        let plan_id = Uuid::new_v4();
        let intent = ExecutionIntent::Speed;

        // For speed, use the "fast" step type to get low-cost/low-latency models
        let fast_model = self.resolve_model_for_step("fast", &intent, model);

        let node_id = Uuid::new_v4();

        let nodes = vec![IRNode {
            id: node_id,
            kind: IRNodeKind::Generate,
            strategy: StrategyKind::Single,
            model: Some(fast_model),
            config: HashMap::new(),
        }];

        // Single node: no edges needed
        let plan = WorkflowIR {
            plan_id,
            nodes,
            edges: vec![],
            metadata: IRMetadata {
                policy_applied: vec!["intent:speed".into()],
                estimated_cost: 0.01,
                estimated_tokens: 1000,
            },
        };

        #[cfg(debug_assertions)]
        Self::validate_plan(&plan);

        plan
    }

    fn build_balanced(&self, model: &str) -> WorkflowIR {
        let plan_id = Uuid::new_v4();
        let intent = ExecutionIntent::Balanced;

        // Balanced: use weighted scoring for model selection
        let gen_model = self.resolve_model_for_step("generate", &intent, model);
        let judge_model = self.resolve_model_for_step("judge", &intent, model);

        let gen1_id = Uuid::new_v4();
        let gen2_id = Uuid::new_v4();
        let judge_id = Uuid::new_v4();

        let nodes = vec![
            IRNode {
                id: gen1_id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(gen_model.clone()),
                config: HashMap::new(),
            },
            IRNode {
                id: gen2_id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(gen_model),
                config: HashMap::new(),
            },
            IRNode {
                id: judge_id,
                kind: IRNodeKind::Judge,
                strategy: StrategyKind::Single,
                model: Some(judge_model),
                config: HashMap::new(),
            },
        ];

        // Chain: gen1 -> gen2 -> judge
        let edges = vec![
            IREdge { from: gen1_id, to: gen2_id, condition: None },
            IREdge { from: gen2_id, to: judge_id, condition: None },
        ];

        let plan = WorkflowIR {
            plan_id,
            nodes,
            edges,
            metadata: IRMetadata {
                policy_applied: vec!["intent:balanced".into()],
                estimated_cost: 0.03,
                estimated_tokens: 3000,
            },
        };

        #[cfg(debug_assertions)]
        Self::validate_plan(&plan);

        plan
    }

    fn build_exhaustive(&self, model: &str) -> WorkflowIR {
        let plan_id = Uuid::new_v4();
        let intent = ExecutionIntent::Exhaustive; // Exhaustive inherits quality intent for scoring

        // Exhaustive: use high-accuracy models for all steps
        let gen_model = self.resolve_model_for_step("generate", &intent, model);
        let judge_model = self.resolve_model_for_step("judge", &intent, model);
        let review_model = self.resolve_model_for_step("review", &intent, model);

        let gen1_id = Uuid::new_v4();
        let gen2_id = Uuid::new_v4();
        let gen3_id = Uuid::new_v4();
        let judge1_id = Uuid::new_v4();
        let reflect_id = Uuid::new_v4();
        let judge2_id = Uuid::new_v4();

        let nodes = vec![
            IRNode {
                id: gen1_id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(gen_model.clone()),
                config: HashMap::new(),
            },
            IRNode {
                id: gen2_id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(gen_model.clone()),
                config: HashMap::new(),
            },
            IRNode {
                id: gen3_id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Single,
                model: Some(gen_model),
                config: HashMap::new(),
            },
            IRNode {
                id: judge1_id,
                kind: IRNodeKind::Judge,
                strategy: StrategyKind::Single,
                model: Some(judge_model.clone()),
                config: HashMap::new(),
            },
            IRNode {
                id: reflect_id,
                kind: IRNodeKind::Generate,
                strategy: StrategyKind::Reflection,
                model: Some(review_model),
                config: HashMap::new(),
            },
            IRNode {
                id: judge2_id,
                kind: IRNodeKind::Judge,
                strategy: StrategyKind::Consensus,
                model: Some(judge_model),
                config: HashMap::new(),
            },
        ];

        // Chain: gen1 -> gen2 -> gen3 -> judge1 -> reflect -> judge2
        let edges = vec![
            IREdge { from: gen1_id, to: gen2_id, condition: None },
            IREdge { from: gen2_id, to: gen3_id, condition: None },
            IREdge { from: gen3_id, to: judge1_id, condition: None },
            IREdge { from: judge1_id, to: reflect_id, condition: None },
            IREdge { from: reflect_id, to: judge2_id, condition: None },
        ];

        let plan = WorkflowIR {
            plan_id,
            nodes,
            edges,
            metadata: IRMetadata {
                policy_applied: vec!["intent:exhaustive".into()],
                estimated_cost: 0.08,
                estimated_tokens: 8000,
            },
        };

        #[cfg(debug_assertions)]
        Self::validate_plan(&plan);

        plan
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

    /// Resolve the best model for a specific step type, given the intent.
    /// Falls back to `fallback` if catalog is empty or has no matches for this step.
    fn resolve_model_for_step(&self, step: &str, intent: &ExecutionIntent, fallback: &str) -> String {
        let catalog = match &self.capability_catalog {
            Some(cat) => cat,
            None => return fallback.to_string(),
        };

        let candidates = catalog.query_by_capability(step);
        if candidates.is_empty() {
            return fallback.to_string();
        }

        // Filter out candidates with NaN or infinite scores to prevent silent ties
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
                // Prioritize reasoning ability
                valid_candidates.iter().copied().max_by(|a, b| {
                    a.capabilities.reasoning_score
                        .partial_cmp(&b.capabilities.reasoning_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            }
            ExecutionIntent::Speed => {
                // TODO: Replace cost proxy with actual latency metrics from ProviderRegistry when available
                // Prioritize low cost (proxy for speed)
                valid_candidates.iter().copied().min_by(|a, b| {
                    a.pricing.input_cost_per_1k
                        .partial_cmp(&b.pricing.input_cost_per_1k)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            }
            ExecutionIntent::Balanced => {
                // Weighted combination of reasoning and coding scores
                valid_candidates.iter().copied().max_by(|a, b| {
                    let score_a = a.capabilities.reasoning_score * 0.5
                        + a.capabilities.coding_score * 0.5;
                    let score_b = b.capabilities.reasoning_score * 0.5
                        + b.capabilities.coding_score * 0.5;
                    score_a
                        .partial_cmp(&score_b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            }
            ExecutionIntent::Constrained { .. } => {
                // For constrained, use balanced scoring
                valid_candidates.iter().copied().max_by(|a, b| {
                    let score_a = a.capabilities.reasoning_score * 0.5
                        + a.capabilities.coding_score * 0.5;
                    let score_b = b.capabilities.reasoning_score * 0.5
                        + b.capabilities.coding_score * 0.5;
                    score_a
                        .partial_cmp(&score_b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            }
        };

        best.map(|c| format!("{}/{}", c.provider_name, c.model_id))
            .unwrap_or_else(|| fallback.to_string())
    }

    /// Debug-only assertion that validates plan integrity.
    /// Catches edge-node mismatches during development before they hit the compiler.
    #[cfg(debug_assertions)]
    fn validate_plan(plan: &WorkflowIR) {
        let node_ids: std::collections::HashSet<_> = plan.nodes.iter().map(|n| n.id).collect();
        for edge in &plan.edges {
            debug_assert!(
                node_ids.contains(&edge.from),
                "Edge references non-existent source node: {:?}",
                edge.from
            );
            debug_assert!(
                node_ids.contains(&edge.to),
                "Edge references non-existent target node: {:?}",
                edge.to
            );
        }
        // Check for duplicate node IDs
        debug_assert_eq!(
            node_ids.len(),
            plan.nodes.len(),
            "Plan contains duplicate node IDs"
        );
    }

    /// Resolves required capability contracts via the `CapabilityResolver`.
    pub fn resolve_capabilities(
        &self,
        resolver: &super::resolver::capability::CapabilityResolver,
        required: Vec<fusion_plugin_api::CapabilityId>,
    ) -> Result<super::resolver::capability::ResolvedCapabilitySet, super::resolver::capability::ResolverError> {
        let reqs = super::resolver::capability::RequirementSet::new(required);
        resolver.resolve(&reqs)
    }
}

#[async_trait]
impl Planner for IntentPlanner {
    async fn plan(
        &self,
        requirements: &Requirements,
        _policies: &[Policy],
        _evidence: Option<&EvidenceSnapshot>,
    ) -> WorkflowIR {
        let model = self.select_model(requirements);

        match &requirements.execution_intent {
            Some(ExecutionIntent::Quality) => self.build_quality(&model),
            Some(ExecutionIntent::Speed) => self.build_speed(&model),
            Some(ExecutionIntent::Balanced) => self.build_balanced(&model),
            Some(ExecutionIntent::Exhaustive) => self.build_exhaustive(&model),
            Some(ExecutionIntent::Constrained { max_cost_usd, .. }) => {
                if let Some(cost) = max_cost_usd {
                    if *cost < 0.02 {
                        return self.build_speed(&model);
                    }
                }
                self.build_balanced(&model)
            }
            None => {
                match requirements.complexity {
                    ComplexityLevel::Critical => self.build_quality(&model),
                    ComplexityLevel::High => self.build_balanced(&model),
                    ComplexityLevel::Medium | ComplexityLevel::Low => self.build_speed(&model),
                }
            }
        }
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
        assert_eq!(ir.metadata.estimated_cost, 0.05);
        assert_eq!(ir.metadata.estimated_tokens, 5000);
        assert!(ir.metadata.policy_applied.contains(&"intent:quality".to_string()));
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
        assert!(ir.metadata.policy_applied.contains(&"intent:speed".to_string()));
        assert_eq!(ir.metadata.estimated_cost, 0.01);
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
        assert!(ir.metadata.policy_applied.contains(&"intent:balanced".to_string()));
        assert_eq!(ir.metadata.estimated_cost, 0.03);
    }

    #[tokio::test]
    async fn test_exhaustive_plan_six_nodes() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Exhaustive));
        let ir = planner.plan(&reqs, &[], None).await;
        assert_eq!(ir.nodes.len(), 6);
        assert!(ir.metadata.policy_applied.contains(&"intent:exhaustive".to_string()));
        assert_eq!(ir.metadata.estimated_cost, 0.08);
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
            max_cost_usd: Some(0.01),
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
            max_cost_usd: Some(0.05),
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
            max_cost_usd: None,
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
            max_cost_usd: Some(0.02),
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
        assert!(ir.edges.is_empty(), "Speed plan should have no edges (single node)");
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
            assert!(node_ids.contains(&edge.from), "Edge from {:?} references invalid node", edge.from);
            assert!(node_ids.contains(&edge.to), "Edge to {:?} references invalid node", edge.to);
        }
    }
}
