use async_trait::async_trait;

use super::Planner;
use crate::types::execution::ExecutionIntent;
use crate::types::{
    ComplexityLevel, EvidenceSnapshot, ModelCatalog, Policy, Requirements,
    WorkflowIR,
};
use crate::capability::CapabilityRegistry;
use crate::planner::PlannerFailure;
use std::sync::Arc;
use parking_lot::RwLock;

pub struct IntentPlanner {
    pub model_catalog: ModelCatalog,
    pub capability_snapshot: Arc<RwLock<fusion_kernel::CapabilityCatalog>>,
}

impl IntentPlanner {
    pub fn new(model_catalog: ModelCatalog) -> Self {
        Self {
            model_catalog,
            capability_snapshot: Arc::new(RwLock::new(fusion_kernel::CapabilityCatalog::default())),
        }
    }

    pub fn with_capability_registry(
        model_catalog: ModelCatalog,
        registry: Arc<dyn CapabilityRegistry>,
    ) -> Self {
        let snapshot = Self::snapshot_from_registry(registry.as_ref());
        Self {
            model_catalog,
            capability_snapshot: Arc::new(RwLock::new(snapshot)),
        }
    }

    fn snapshot_from_registry(registry: &dyn CapabilityRegistry) -> fusion_kernel::CapabilityCatalog {
        let mut catalog = std::collections::HashMap::new();
        for contract in registry.list() {
            catalog.insert(contract.id.as_str().to_string(), Vec::new());
        }
        fusion_kernel::CapabilityCatalog { catalog }
    }

    pub fn update_capability_registry(&self, registry: &dyn CapabilityRegistry) {
        *self.capability_snapshot.write() = Self::snapshot_from_registry(registry);
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
        requirements: &Requirements,
        policies: &[Policy],
        policy_version: u64,
        evidence: Option<&EvidenceSnapshot>,
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

        let mut required_capabilities = Vec::new();
        if let Some(model_requirements) = &requirements.model_requirements {
            if model_requirements.requires_tools { required_capabilities.push("ToolCalling".into()); }
            if model_requirements.requires_vision { required_capabilities.push("Vision".into()); }
            if model_requirements.requires_streaming { required_capabilities.push("Streaming".into()); }
        }
        let (avg_latency_ms, error_rate, healthy_provider_count) = evidence.map(|snapshot| {
            let avg_latency_ms = if snapshot.avg_latencies.is_empty() { 0 } else {
                (snapshot.avg_latencies.values().sum::<f64>() / snapshot.avg_latencies.len() as f64).max(0.0) as u64
            };
            let healthy = snapshot.success_rates.values().filter(|rate| **rate > 0.0).count();
            let error_rate = if snapshot.success_rates.is_empty() { 0.0 } else {
                1.0 - snapshot.success_rates.values().sum::<f64>() / snapshot.success_rates.len() as f64
            };
            (avg_latency_ms, error_rate.clamp(0.0, 1.0), healthy)
        }).unwrap_or((0, 0.0, 0));

        let requested_strategy_str = requirements.requested_strategy.as_ref().map(|s| s.kind.clone());
        let req = fusion_planner::PlanningRequest {
            intent: crates_intent.clone(),
            user_prompt: requirements.original_text.clone(),
            requested_model: requirements.requested_model.clone(),
            requested_strategy: requested_strategy_str,
            strategy_config: None,
            requirements: fusion_planner::RequirementsSnapshot {
                complexity: format!("{:?}", requirements.complexity),
                execution_intent: Some(crates_intent),
                required_capabilities,
            },
            policies: fusion_planner::PolicySnapshot {
                version: policy_version,
                policies: policy_declarations,
                created_at: 0,
            },
            capability_catalog: fusion_planner::CapabilityCatalogSnapshot::new(self.capability_snapshot.read().clone()),
            model_catalog: fusion_planner::ModelCatalogSnapshot::new(Self::to_fusion_core_catalog(&self.model_catalog)),
            telemetry: fusion_planner::RoutingTelemetrySnapshot { avg_latency_ms, error_rate, healthy_provider_count },
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
        evidence: Option<&EvidenceSnapshot>,
    ) -> Result<WorkflowIR, PlannerFailure> {
        self.plan_with_policy_version(requirements, policies, evidence, 0).await
    }

    async fn plan_with_policy_version(
        &self,
        requirements: &Requirements,
        policies: &[Policy],
        evidence: Option<&EvidenceSnapshot>,
        policy_version: u64,
    ) -> Result<WorkflowIR, PlannerFailure> {

        let intent = requirements.execution_intent.clone().unwrap_or({
            match requirements.complexity {
                ComplexityLevel::Critical => ExecutionIntent::Quality,
                ComplexityLevel::High => ExecutionIntent::Balanced,
                ComplexityLevel::Medium | ComplexityLevel::Low => ExecutionIntent::Speed,
            }
        });
        // A planner failure is a service-capacity problem, not a panic:
        // surface it as a retryable 503 instead of dropping the connection.
        self.plan_from_crates(&intent, requirements, policies, policy_version, evidence)
            .map_err(PlannerFailure)
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
            requested_model: None,
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
    async fn test_planning_is_snapshot_driven() {
        let planner = make_planner();
        let ir = planner.plan(&make_reqs(Some(ExecutionIntent::Balanced)), &[], None).await.unwrap();
        assert!(ir.nodes.iter().all(|n| n.model.is_some()));
        assert!(ir.metadata.policy_applied.contains(&"planner:synthesized".to_string()));
    }

    #[tokio::test]
    async fn test_quality_plan_node_kinds() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Quality));
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        assert_eq!(ir.nodes.len(), 5);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
        assert_eq!(ir.nodes[0].strategy, StrategyKind::Single);
    }

    #[tokio::test]
    async fn test_speed_plan_single_node() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Speed));
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
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
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        assert_eq!(ir.nodes.len(), 3);
        assert_eq!(ir.nodes[0].kind, IRNodeKind::Generate);
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
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        assert_eq!(ir.nodes.len(), 6);
        assert!(ir
            .metadata
            .policy_applied
            .contains(&"intent:exhaustive".to_string()));
        assert_eq!(ir.metadata.estimated_cost, NanoUSD::from_nanos(60_000_000));
        assert_eq!(ir.metadata.estimated_tokens, 6000);
    }

    #[tokio::test]
    async fn test_exhaustive_plan_ends_with_consensus_judge() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Exhaustive));
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        let last = ir.nodes.last().unwrap();
        assert_eq!(last.kind, IRNodeKind::Generate);
        assert_eq!(last.strategy, StrategyKind::Single);
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
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
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
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
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
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        assert_eq!(ir.nodes.len(), 1);
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
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        assert_eq!(ir.nodes.len(), 3);
    }

    #[tokio::test]
    async fn test_no_intent_critical_complexity_returns_quality() {
        let planner = make_planner();
        let mut reqs = make_reqs(None);
        reqs.complexity = ComplexityLevel::Critical;
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        assert_eq!(ir.nodes.len(), 5);
    }

    #[tokio::test]
    async fn test_no_intent_high_complexity_returns_balanced() {
        let planner = make_planner();
        let mut reqs = make_reqs(None);
        reqs.complexity = ComplexityLevel::High;
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        assert_eq!(ir.nodes.len(), 3);
    }

    #[tokio::test]
    async fn test_no_intent_medium_complexity_returns_speed() {
        let planner = make_planner();
        let mut reqs = make_reqs(None);
        reqs.complexity = ComplexityLevel::Medium;
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        assert_eq!(ir.nodes.len(), 1);
    }

    #[tokio::test]
    async fn test_no_intent_low_complexity_returns_speed() {
        let planner = make_planner();
        let mut reqs = make_reqs(None);
        reqs.complexity = ComplexityLevel::Low;
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        assert_eq!(ir.nodes.len(), 1);
    }

    #[tokio::test]
    async fn test_planner_selects_model_inside_contract() {
        let planner = make_planner();
        let ir = planner.plan(&make_reqs(None), &[], None).await.unwrap();
        assert!(ir.nodes.iter().all(|node| node.model.is_some()));
    }

    #[tokio::test]
    async fn test_each_intent_produces_distinct_plan_ids() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Quality));
        let ir1 = planner.plan(&reqs, &[], None).await.unwrap();
        let reqs = make_reqs(Some(ExecutionIntent::Speed));
        let ir2 = planner.plan(&reqs, &[], None).await.unwrap();
        assert_ne!(ir1.plan_id, ir2.plan_id);
    }

    #[tokio::test]
    async fn test_plan_nodes_have_unique_ids() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Exhaustive));
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        let mut ids: Vec<_> = ir.nodes.iter().map(|n| n.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ir.nodes.len());
    }

    #[tokio::test]
    async fn test_speed_plan_has_no_edges() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Speed));
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        assert!(
            ir.edges.is_empty(),
            "Speed plan should have no edges (single node)"
        );
    }

    #[tokio::test]
    async fn test_quality_plan_has_edges() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Quality));
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        assert!(!ir.edges.is_empty(), "multi-stage plan must have edges");
    }

    #[tokio::test]
    async fn test_balanced_plan_has_edges() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Balanced));
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        assert!(!ir.edges.is_empty(), "multi-stage plan must have edges");
    }

    #[tokio::test]
    async fn test_exhaustive_plan_has_edges() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Exhaustive));
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
        assert!(!ir.edges.is_empty(), "multi-stage plan must have edges");
    }

    #[tokio::test]
    async fn test_edges_reference_valid_node_ids() {
        let planner = make_planner();
        let reqs = make_reqs(Some(ExecutionIntent::Quality));
        let ir = planner.plan(&reqs, &[], None).await.unwrap();
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


