pub mod capability;
pub mod planning_request;

pub use planning_request::*;

use fusion_core::{ModelCatalog, NanoUSD, PlatformError};
use fusion_ir::{WorkflowBuilder, WorkflowIR, WorkflowMetadata, WorkflowNodeKind};
use fusion_kernel::{CapabilityCatalog, CapabilitySystem};
use std::collections::BTreeMap;

/// Recursive canonical JSON serialization: object keys are sorted at every
/// depth so the planner identity is immune to map iteration order (mirrors
/// `canonical_json` in the compiler crate).
pub(crate) fn canonical_json_value(value: &serde_json::Value) -> String {
    fn canonicalize(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for key in keys {
                    out.insert(key.clone(), canonicalize(&map[key]));
                }
                serde_json::Value::Object(out)
            }
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(canonicalize).collect())
            }
            other => other.clone(),
        }
    }
    serde_json::to_string(&canonicalize(value)).expect("value always serializes")
}

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
        // 2. Select a strategy constraint. Strategy graph expansion belongs to
        // the compiler; the planner emits a strategy-bearing node rather than
        // duplicating each strategy's topology here.
        let stages: Vec<(String, WorkflowNodeKind, String, String)> =
            if let Some(ref strat) = req.requested_strategy {
                let capability = req
                    .requirements
                    .required_capabilities
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "CodeGeneration".into());
                vec![(
                    "n1".into(),
                    WorkflowNodeKind::Task,
                    capability,
                    strat.clone(),
                )]
            } else {
                let mut capabilities = req.requirements.required_capabilities.clone();
                if capabilities.is_empty() {
                    match intent {
                        ExecutionIntent::Speed => capabilities.push("CodeGeneration".into()),
                        ExecutionIntent::Balanced => {
                            capabilities.extend([
                                "CodeGeneration".into(),
                                "CodeGeneration".into(),
                                "CodeReview".into(),
                            ]);
                        }
                        ExecutionIntent::Quality => {
                            capabilities.extend([
                                "CodeGeneration".into(),
                                "CodeReview".into(),
                                "CodeGeneration".into(),
                                "CodeReview".into(),
                                "CodeGeneration".into(),
                            ]);
                        }
                        ExecutionIntent::Exhaustive => {
                            capabilities.extend([
                                "CodeGeneration".into(),
                                "CodeReview".into(),
                                "CodeGeneration".into(),
                                "CodeReview".into(),
                                "CodeGeneration".into(),
                                "Judgment".into(),
                            ]);
                        }
                        ExecutionIntent::Constrained { max_cost } => {
                            if max_cost
                                .as_ref()
                                .is_some_and(|c| c.as_nanos() >= 20_000_000)
                            {
                                capabilities.extend([
                                    "CodeGeneration".into(),
                                    "CodeGeneration".into(),
                                    "CodeReview".into(),
                                ]);
                            } else {
                                capabilities.push("CodeGeneration".into());
                            }
                        }
                    }
                }
                let limit = match intent {
                    ExecutionIntent::Speed => 1,
                    ExecutionIntent::Constrained {
                        max_cost: Some(cost),
                    } if cost.as_nanos() < 20_000_000 => 1,
                    _ => capabilities.len().max(1),
                };
                // A zero healthy-provider count means telemetry is unavailable,
                // not that the planner must collapse to a single stage.
                let telemetry_limit = if req.telemetry.healthy_provider_count == 0 {
                    capabilities.len().max(1)
                } else {
                    req.telemetry.healthy_provider_count
                };
                capabilities.truncate(limit.min(telemetry_limit));
                capabilities
                    .into_iter()
                    .enumerate()
                    .map(|(i, capability)| {
                        let kind = if capability.to_ascii_lowercase().contains("review") {
                            WorkflowNodeKind::Review
                        } else {
                            WorkflowNodeKind::Task
                        };
                        (
                            format!("n{}", i + 1),
                            kind,
                            capability,
                            "Single".to_string(),
                        )
                    })
                    .collect()
            };

        // 3. Score and select models for each stage
        //
        // Identity inputs must cover every request field that changes the plan
        // shape: requested_strategy/requested_model/strategy_config alter the
        // emitted nodes, so distinct requests must never collide on plan_id.
        // strategy_config is hashed as canonical JSON (recursively sorted keys).
        let strategy_config_canonical = req
            .strategy_config
            .as_ref()
            .map(|map| {
                canonical_json_value(&serde_json::Value::Object(
                    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                ))
            })
            .unwrap_or_else(|| "null".to_string());
        let identity = format!(
            "{:?}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            req.intent,
            req.user_prompt,
            req.requirements.complexity,
            req.requirements.required_capabilities.join(","),
            req.policies.version,
            req.model_catalog.catalog.code,
            req.telemetry.avg_latency_ms,
            req.telemetry.healthy_provider_count,
            req.requested_strategy.as_deref().unwrap_or(""),
            req.requested_model.as_deref().unwrap_or(""),
            strategy_config_canonical,
        );
        let workflow_id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, identity.as_bytes());
        let mut builder = WorkflowBuilder::new().with_workflow_id(workflow_id);

        for (idx, (node_id, kind, capability, strategy)) in stages.iter().enumerate() {
            let model = self.resolve_stage_model(capability, kind, req);

            let mut config = BTreeMap::new();
            config.insert("strategy".to_string(), serde_json::json!(strategy));
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
                .add_node_with_model(node_id, *kind, Some(capability), Some(model), config)
                .map_err(|e| PlatformError::Planner {
                    code: "BUILDER_ERR".to_string(),
                    message: format!("Failed to add node {node_id}: {e}"),
                    recovery_suggestion: "Check workflow node topology".into(),
                })?;
        }

        // 4. Construct edges sequentially across the stages
        for i in 0..stages.len().saturating_sub(1) {
            let from = stages[i].0.as_str();
            let to = stages[i + 1].0.as_str();
            builder = builder
                .sequential(from, to)
                .map_err(|e| PlatformError::Planner {
                    code: "EDGE_ERR".to_string(),
                    message: format!("Failed to wire edge {from} -> {to}: {e}"),
                    recovery_suggestion: "Ensure source and target nodes exist".into(),
                })?;
        }

        // 5. Calculate metadata (estimated cost, estimated tokens, policy tracking)
        let estimated_tokens = (stages.len() as u64).saturating_mul(1000);
        let estimated_cost = NanoUSD::from_nanos(estimated_tokens.saturating_mul(10_000));
        let mut policy_applied = vec!["planner:synthesized".to_string()];
        let intent_label = match intent {
            ExecutionIntent::Quality => "quality",
            ExecutionIntent::Speed => "speed",
            ExecutionIntent::Balanced => "balanced",
            ExecutionIntent::Exhaustive => "exhaustive",
            ExecutionIntent::Constrained { .. } => "constrained",
        };
        policy_applied.push(format!("intent:{intent_label}"));
        for pol in &req.policies.policies {
            policy_applied.push(pol.name.clone());
        }

        let metadata = WorkflowMetadata {
            policy_applied,
            policy_version: req.policies.version,
            estimated_cost,
            estimated_tokens,
        };

        builder
            .metadata(metadata)
            .build()
            .map_err(|e| PlatformError::Planner {
                code: "BUILD_ERR".to_string(),
                message: format!("Failed to finalize workflow IR: {e}"),
                recovery_suggestion: "Validate workflow DAG acyclicity and metadata".into(),
            })
    }

    /// Resolves target model per stage using snapshot constraints, capabilities, and telemetry.
    fn resolve_stage_model(
        &self,
        _capability: &str,
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

    pub fn plan_with_intent(
        &self,
        intent_text: &str,
        execution_intent: ExecutionIntent,
    ) -> Result<WorkflowIR, PlatformError> {
        if intent_text.is_empty() {
            return Err(PlatformError::Planner {
                code: "EMPTY_INTENT".to_string(),
                message: "Intent cannot be empty".to_string(),
                recovery_suggestion: "Provide a valid natural language prompt or workflow spec"
                    .to_string(),
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
        let quality_ir = planner
            .plan_with_intent("Build web application", ExecutionIntent::Quality)
            .expect("Plan");
        assert_eq!(quality_ir.nodes().len(), 5);

        let speed_ir = planner
            .plan_with_intent("Quick fix", ExecutionIntent::Speed)
            .expect("Plan");
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
        assert!(ir
            .metadata()
            .policy_applied
            .contains(&"deny-unauth".to_string()));
        assert_eq!(ir.metadata().policy_version, 1);
        for node in ir.nodes() {
            assert_eq!(node.selected_model(), Some("custom-llm"));
        }
    }

    #[test]
    fn identical_snapshot_requests_produce_identical_artifacts() {
        let planner = IntentPlanner::new(ModelCatalog::default());
        let req = PlanningRequest {
            intent: ExecutionIntent::Balanced,
            user_prompt: "stable plan".into(),
            requested_model: None,
            requested_strategy: None,
            strategy_config: None,
            requirements: RequirementsSnapshot {
                required_capabilities: vec!["CodeGeneration".into()],
                ..Default::default()
            },
            policies: PolicySnapshot {
                version: 7,
                ..Default::default()
            },
            capability_catalog: CapabilityCatalogSnapshot::default(),
            model_catalog: ModelCatalogSnapshot::default(),
            telemetry: RoutingTelemetrySnapshot::default(),
        };
        let first = planner.plan(&req).unwrap().to_canonical_json().unwrap();
        let second = planner.plan(&req).unwrap().to_canonical_json().unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn repeat_calls_are_fully_deterministic_including_plan_id() {
        let planner = IntentPlanner::new(ModelCatalog::default());
        let req = PlanningRequest {
            intent: ExecutionIntent::Quality,
            user_prompt: "deterministic identity".into(),
            requested_model: None,
            requested_strategy: Some("Consensus".into()),
            strategy_config: Some(BTreeMap::from([
                ("count".to_string(), serde_json::json!(3)),
                ("members".to_string(), serde_json::json!(["a", "b"])),
            ])),
            requirements: RequirementsSnapshot::default(),
            policies: PolicySnapshot::default(),
            capability_catalog: CapabilityCatalogSnapshot::default(),
            model_catalog: ModelCatalogSnapshot::default(),
            telemetry: RoutingTelemetrySnapshot::default(),
        };
        let first = planner.plan(&req).unwrap();
        let second = planner.plan(&req).unwrap();
        assert_eq!(
            first.workflow_id(),
            second.workflow_id(),
            "repeat call must derive the same plan_id"
        );
        assert_eq!(
            first.to_canonical_json().unwrap(),
            second.to_canonical_json().unwrap()
        );
    }

    #[test]
    fn different_requested_strategy_produces_different_plan_ids() {
        let planner = IntentPlanner::new(ModelCatalog::default());
        let make_req = |strategy: Option<String>| PlanningRequest {
            intent: ExecutionIntent::Balanced,
            user_prompt: "same prompt".into(),
            requested_model: None,
            requested_strategy: strategy.clone(),
            strategy_config: None,
            requirements: RequirementsSnapshot {
                required_capabilities: vec!["CodeGeneration".into()],
                ..Default::default()
            },
            policies: PolicySnapshot::default(),
            capability_catalog: CapabilityCatalogSnapshot::default(),
            model_catalog: ModelCatalogSnapshot::default(),
            telemetry: RoutingTelemetrySnapshot::default(),
        };

        let without = planner.plan(&make_req(None)).unwrap();
        let consensus = planner.plan(&make_req(Some("Consensus".into()))).unwrap();
        let chain = planner.plan(&make_req(Some("Chain".into()))).unwrap();

        assert_ne!(
            without.workflow_id(),
            consensus.workflow_id(),
            "requested_strategy must be part of the workflow identity"
        );
        assert_ne!(consensus.workflow_id(), chain.workflow_id());
    }

    #[test]
    fn different_requested_model_or_config_produces_different_plan_ids() {
        let planner = IntentPlanner::new(ModelCatalog::default());
        let base = || PlanningRequest {
            intent: ExecutionIntent::Balanced,
            user_prompt: "same prompt".into(),
            requested_model: None,
            requested_strategy: Some("Consensus".into()),
            strategy_config: None,
            requirements: RequirementsSnapshot::default(),
            policies: PolicySnapshot::default(),
            capability_catalog: CapabilityCatalogSnapshot::default(),
            model_catalog: ModelCatalogSnapshot::default(),
            telemetry: RoutingTelemetrySnapshot::default(),
        };

        let mut with_model = base();
        with_model.requested_model = Some("custom-llm".into());

        let mut with_config = base();
        with_config.strategy_config = Some(BTreeMap::from([(
            "count".to_string(),
            serde_json::json!(5),
        )]));

        let plain = planner.plan(&base()).unwrap();
        assert_ne!(
            plain.workflow_id(),
            planner.plan(&with_model).unwrap().workflow_id()
        );
        assert_ne!(
            plain.workflow_id(),
            planner.plan(&with_config).unwrap().workflow_id()
        );

        // Config key insertion order must not change the identity.
        let mut config_reordered = base();
        config_reordered.strategy_config = Some(BTreeMap::from([
            ("alpha".to_string(), serde_json::json!({"z": 1, "a": 2})),
            ("count".to_string(), serde_json::json!(5)),
        ]));
        let first = planner.plan(&config_reordered).unwrap();
        let second = planner.plan(&config_reordered).unwrap();
        assert_eq!(first.workflow_id(), second.workflow_id());
    }
}
