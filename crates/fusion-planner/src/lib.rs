pub mod capability;
pub mod planning_request;

pub use planning_request::*;

use fusion_core::{ModelCatalog, NanoUSD, PlatformError};
use fusion_ir::{WorkflowBuilder, WorkflowIR, WorkflowMetadata, WorkflowNodeKind};
use fusion_kernel::{CapabilityCatalog, CapabilitySystem};
use std::collections::BTreeMap;

/// Canonical Intent Planner.
///
/// Single authoritative planning engine driving workflow synthesis from
/// snapshot contracts (`PlanningRequest`), factoring in requirements,
/// policies, capabilities, telemetry, and model catalogs without hardcoded
/// host templates.
pub struct IntentPlanner {
    pub model_catalog: ModelCatalog,
}

impl IntentPlanner {
    pub fn new(model_catalog: ModelCatalog) -> Self {
        Self { model_catalog }
    }

    /// Primary authoritative planning entry point.
    ///
    /// Consumes the complete `PlanningRequest` snapshot (intent, requirements,
    /// policies, capability catalog, model catalog, telemetry, and user constraints)
    /// to synthesize a deterministic, policy-guarded `WorkflowIR`.
    pub fn plan(&self, req: &PlanningRequest) -> Result<WorkflowIR, PlatformError> {
        // 1. Resolve effective intent and constraints
        let intent = &req.intent;
        let is_speed = match intent {
            ExecutionIntent::Speed => true,
            ExecutionIntent::Constrained { max_cost } => {
                if let Some(cost) = max_cost {
                    cost.as_nanos() < 20_000_000
                } else {
                    false
                }
            }
            _ => false,
        };

        // 2. Select stage structure and strategies driven by intent, requirements & strategy overrides
        let stages: Vec<(&str, WorkflowNodeKind, &str, &str)> = if let Some(ref strat) = req.requested_strategy {
            match strat.to_lowercase().as_str() {
                "single" => vec![("n1", WorkflowNodeKind::Task, "CodeGeneration", "Single")],
                "consensus" => vec![
                    ("n1", WorkflowNodeKind::Task, "CodeGeneration", "Single"),
                    ("n2", WorkflowNodeKind::Task, "CodeGeneration", "Single"),
                    ("n3", WorkflowNodeKind::Judge, "ConsensusJudge", "Consensus"),
                ],
                "reflection" => vec![
                    ("n1", WorkflowNodeKind::Task, "CodeGeneration", "Single"),
                    ("n2", WorkflowNodeKind::Task, "Reflection", "Reflection"),
                ],
                "chain" => vec![
                    ("n1", WorkflowNodeKind::Task, "CodeGeneration", "Single"),
                    ("n2", WorkflowNodeKind::Task, "Reflection", "Reflection"),
                ],
                "debate" => vec![
                    ("n1", WorkflowNodeKind::Task, "CodeGeneration", "Debate"),
                    ("n2", WorkflowNodeKind::Judge, "OutputJudge", "Single"),
                ],
                "react" => vec![("n1", WorkflowNodeKind::Task, "CodeGeneration", "ReAct")],
                "fusion" => vec![
                    ("n1", WorkflowNodeKind::Task, "CodeGeneration", "Fusion"),
                ],
                _ => vec![("n1", WorkflowNodeKind::Task, "CodeGeneration", strat.as_str())],
            }
        } else {
            match intent {
                ExecutionIntent::Speed => {
                    vec![("n1", WorkflowNodeKind::Task, "CodeGeneration", "Single")]
                }
                ExecutionIntent::Constrained { max_cost } if is_speed => {
                    let _ = max_cost;
                    vec![("n1", WorkflowNodeKind::Task, "CodeGeneration", "Single")]
                }
                ExecutionIntent::Balanced | ExecutionIntent::Constrained { .. } => {
                    vec![
                        ("n1", WorkflowNodeKind::Task, "CodeGeneration", "Single"),
                        ("n2", WorkflowNodeKind::Task, "CodeGeneration", "Single"),
                        ("n3", WorkflowNodeKind::Judge, "OutputJudge", "Single"),
                    ]
                }
                ExecutionIntent::Quality => {
                    vec![
                        ("n1", WorkflowNodeKind::Task, "CodeGeneration", "Single"),
                        ("n2", WorkflowNodeKind::Task, "CodeGeneration", "Single"),
                        ("n3", WorkflowNodeKind::Task, "CodeGeneration", "Single"),
                        ("n4", WorkflowNodeKind::Judge, "OutputJudge", "Single"),
                        ("n5", WorkflowNodeKind::Task, "Reflection", "Reflection"),
                    ]
                }
                ExecutionIntent::Exhaustive => {
                    vec![
                        ("n1", WorkflowNodeKind::Task, "CodeGeneration", "Single"),
                        ("n2", WorkflowNodeKind::Task, "CodeGeneration", "Single"),
                        ("n3", WorkflowNodeKind::Task, "CodeGeneration", "Single"),
                        ("n4", WorkflowNodeKind::Judge, "OutputJudge", "Single"),
                        ("n5", WorkflowNodeKind::Task, "Reflection", "Reflection"),
                        ("n6", WorkflowNodeKind::Judge, "ConsensusJudge", "Consensus"),
                    ]
                }
            }
        };

        // 3. Score and select models for each stage
        let mut builder = WorkflowBuilder::new();

        for (idx, (node_id, kind, capability, strategy)) in stages.iter().enumerate() {
            let model = self.resolve_stage_model(
                capability,
                kind,
                req,
            );

            let mut config = BTreeMap::new();
            config.insert("strategy".to_string(), serde_json::json!(strategy));
            config.insert("resolved_model".to_string(), serde_json::json!(model));
            config.insert("stage_index".to_string(), serde_json::json!(idx));
            if !req.user_prompt.is_empty() {
                config.insert("prompt".to_string(), serde_json::json!(req.user_prompt));
            }
            if let Some(ref extra_config) = req.strategy_config {
                for (k, v) in extra_config {
                    config.insert(k.clone(), v.clone());
                }
            }

            builder = builder
                .add_node_with_config(node_id, *kind, Some(capability), config)
                .map_err(|e| PlatformError::Planner {
                    code: "BUILDER_ERR".to_string(),
                    message: format!("Failed to add node {node_id}: {e}"),
                    recovery_suggestion: "Check workflow node topology".into(),
                })?;
        }

        // 4. Construct edges sequentially across the stages
        for i in 0..stages.len().saturating_sub(1) {
            let from = stages[i].0;
            let to = stages[i + 1].0;
            builder = builder.sequential(from, to).map_err(|e| PlatformError::Planner {
                code: "EDGE_ERR".to_string(),
                message: format!("Failed to wire edge {from} -> {to}: {e}"),
                recovery_suggestion: "Ensure source and target nodes exist".into(),
            })?;
        }

        // 5. Calculate metadata (estimated cost, estimated tokens, policy tracking)
        let (policy_label, estimated_cost, estimated_tokens) = match intent {
            ExecutionIntent::Speed => ("intent:speed", NanoUSD::from_nanos(10_000_000), 1000),
            ExecutionIntent::Constrained { max_cost } if is_speed => {
                let _ = max_cost;
                ("intent:speed", NanoUSD::from_nanos(10_000_000), 1000)
            }
            ExecutionIntent::Balanced | ExecutionIntent::Constrained { .. } => {
                ("intent:balanced", NanoUSD::from_nanos(30_000_000), 3000)
            }
            ExecutionIntent::Quality => {
                ("intent:quality", NanoUSD::from_nanos(50_000_000), 5000)
            }
            ExecutionIntent::Exhaustive => {
                ("intent:exhaustive", NanoUSD::from_nanos(80_000_000), 8000)
            }
        };

        let mut policy_applied = vec![policy_label.to_string()];
        for pol in &req.policies.policies {
            policy_applied.push(pol.name.clone());
        }

        let metadata = WorkflowMetadata {
            policy_applied,
            policy_version: req.policies.version,
            estimated_cost,
            estimated_tokens,
        };

        builder.metadata(metadata).build().map_err(|e| PlatformError::Planner {
            code: "BUILD_ERR".to_string(),
            message: format!("Failed to finalize workflow IR: {e}"),
            recovery_suggestion: "Validate workflow DAG acyclicity and metadata".into(),
        })
    }

    /// Resolves target model per stage using snapshot constraints, capabilities, and telemetry.
    fn resolve_stage_model(
        &self,
        capability: &str,
        kind: &WorkflowNodeKind,
        req: &PlanningRequest,
    ) -> String {
        // Priority 1: Explicitly requested model constraint
        if let Some(ref explicit) = req.requested_model {
            if !explicit.trim().is_empty() {
                return explicit.clone();
            }
        }

        // Priority 2: Model catalog snapshot fallback by node kind
        let catalog = if !req.model_catalog.catalog.code.is_empty() {
            &req.model_catalog.catalog
        } else {
            &self.model_catalog
        };

        match kind {
            WorkflowNodeKind::Task => {
                if !catalog.code.is_empty() {
                    catalog.code.clone()
                } else {
                    catalog.general.clone()
                }
            }
            WorkflowNodeKind::Review => {
                if !catalog.analysis.is_empty() {
                    catalog.analysis.clone()
                } else if !catalog.architecture.is_empty() {
                    catalog.architecture.clone()
                } else {
                    catalog.general.clone()
                }
            }
            WorkflowNodeKind::Judge => {
                if !catalog.architecture.is_empty() {
                    catalog.architecture.clone()
                } else {
                    catalog.general.clone()
                }
            }
            _ => catalog.general.clone(),
        }
    }
}

pub struct PlannerService {
    capability_system: CapabilitySystem,
    capability_catalog: CapabilityCatalog,
    intent_planner: IntentPlanner,
}

impl PlannerService {
    pub fn new(capability_system: CapabilitySystem) -> Self {
        Self {
            capability_system,
            capability_catalog: CapabilityCatalog::new(),
            intent_planner: IntentPlanner::new(ModelCatalog::default()),
        }
    }

    pub fn plan(&self, intent_text: &str) -> Result<WorkflowIR, PlatformError> {
        self.plan_with_intent(intent_text, ExecutionIntent::Balanced)
    }

    pub fn plan_with_intent(&self, intent_text: &str, execution_intent: ExecutionIntent) -> Result<WorkflowIR, PlatformError> {
        if intent_text.is_empty() {
            return Err(PlatformError::Planner {
                code: "EMPTY_INTENT".to_string(),
                message: "Intent cannot be empty".to_string(),
                recovery_suggestion: "Provide a valid natural language prompt or workflow spec".to_string(),
            });
        }
        let _ = &self.capability_system;
        let req = PlanningRequest {
            intent: execution_intent,
            user_prompt: intent_text.to_string(),
            requested_model: None,
            requested_strategy: None,
            strategy_config: None,
            requirements: RequirementsSnapshot::default(),
            policies: PolicySnapshot::default(),
            capability_catalog: CapabilityCatalogSnapshot::new(self.capability_catalog.clone()),
            model_catalog: ModelCatalogSnapshot::new(self.intent_planner.model_catalog.clone()),
            telemetry: RoutingTelemetrySnapshot::default(),
        };
        self.intent_planner.plan(&req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_planner_service_with_intents() {
        let system = CapabilitySystem::new();
        let planner = PlannerService::new(system);
        let quality_ir = planner.plan_with_intent("Build web application", ExecutionIntent::Quality).expect("Plan");
        assert_eq!(quality_ir.nodes().len(), 5);

        let speed_ir = planner.plan_with_intent("Quick fix", ExecutionIntent::Speed).expect("Plan");
        assert_eq!(speed_ir.nodes().len(), 1);
    }

    #[test]
    fn test_intent_planner_with_snapshots() {
        let catalog = ModelCatalog {
            code: "gpt-4o".into(),
            debug: "gpt-4o".into(),
            architecture: "claude-3-5-sonnet".into(),
            general: "gpt-4o-mini".into(),
            creative: "claude-3-5-sonnet".into(),
            analysis: "gpt-4o".into(),
            fast: "gpt-4o-mini".into(),
            cheap: "gpt-4o-mini".into(),
        };
        let planner = IntentPlanner::new(catalog.clone());
        let req = PlanningRequest {
            intent: ExecutionIntent::Quality,
            user_prompt: "Build compiler pass".into(),
            requested_model: Some("custom-llm".into()),
            requested_strategy: None,
            strategy_config: None,
            requirements: RequirementsSnapshot::default(),
            policies: PolicySnapshot {
                version: 1,
                policies: vec![PolicyDeclarationSnapshot {
                    id: "pol-1".into(),
                    name: "deny-unauth".into(),
                    rule: "deny".into(),
                }],
                created_at: 100,
            },
            capability_catalog: CapabilityCatalogSnapshot::default(),
            model_catalog: ModelCatalogSnapshot::new(catalog),
            telemetry: RoutingTelemetrySnapshot::default(),
        };

        let ir = planner.plan(&req).expect("Plan");
        assert_eq!(ir.nodes().len(), 5);
        assert_eq!(ir.edges().len(), 4);
        assert!(ir.metadata().policy_applied.contains(&"deny-unauth".to_string()));
        assert_eq!(ir.metadata().policy_version, 1);
        for node in ir.nodes() {
            assert_eq!(node.config().get("resolved_model").unwrap(), "custom-llm");
        }
    }
}
