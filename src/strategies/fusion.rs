use std::time::Duration;

use super::{Parallelism, StreamingMode, Strategy, StrategyDescriptor};
use crate::compiler::context::CompilationContext;
use crate::compiler::diagnostics::CompilerDiagnostic;
use crate::compiler::ir::{
    BarrierFailurePolicy, PrimitiveGraph, PrimitiveNode, PrimitiveNodeKind, ReducerMode, StrategyIR,
};
use crate::types::{
    ArtifactKind, RetryPolicy,
};

#[derive(Debug, Clone)]
pub enum ModelCapability {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Default)]
pub struct ModelAvailability {
    pub has_high_capability: bool,
    pub has_medium_capability: bool,
    pub eligible_models: Vec<String>,
}

impl ModelAvailability {
    pub fn from_models(models: &[String]) -> Self {
        let high_keywords = ["gpt-4", "claude-opus", "gemini-ultra", "claude-3.5", "o1", "o3"];
        let medium_keywords = ["gpt-4o-mini", "claude-sonnet", "gemini-pro", "claude-haiku"];
        let matches_keyword = |model: &str, keyword: &str| {
            if model == keyword {
                return true;
            }
            if let Some(rest) = model.strip_prefix(keyword) {
                if rest.starts_with('-') || rest.starts_with('.') {
                    return true;
                }
            }
            let mut search_from = 0;
            while let Some(idx) = model[search_from..].find(keyword) {
                let abs_idx = search_from + idx;
                let preceded_by_dash = abs_idx > 0 && model.as_bytes()[abs_idx - 1] == b'-';
                let end_idx = abs_idx + keyword.len();
                let followed_by_dash_or_dot = end_idx < model.len()
                    && (model.as_bytes()[end_idx] == b'-' || model.as_bytes()[end_idx] == b'.');
                if preceded_by_dash && followed_by_dash_or_dot {
                    return true;
                }
                search_from = abs_idx + 1;
            }
            false
        };
        let has_high = models.iter().any(|m| high_keywords.iter().any(|k| matches_keyword(m, k)));
        let has_medium = models.iter().any(|m| medium_keywords.iter().any(|k| matches_keyword(m, k)));
        Self {
            has_high_capability: has_high,
            has_medium_capability: has_medium || has_high,
            eligible_models: models.to_vec(),
        }
    }

    pub fn capability(&self) -> ModelCapability {
        if self.has_high_capability {
            ModelCapability::High
        } else if self.has_medium_capability {
            ModelCapability::Medium
        } else {
            ModelCapability::Low
        }
    }
}

pub struct FusionStrategy {
    pub sub_strategies: Vec<Box<dyn Strategy>>,
    pub model_hints: Option<ModelAvailability>,
}

impl FusionStrategy {
    pub fn new(sub_strategies: Vec<Box<dyn Strategy>>) -> Self {
        Self {
            sub_strategies,
            model_hints: None,
        }
    }

    pub fn with_model_hints(mut self, hints: ModelAvailability) -> Self {
        self.model_hints = Some(hints);
        self
    }

    fn active_count(&self) -> usize {
        match &self.model_hints {
            Some(h) if !h.has_high_capability && !h.has_medium_capability => {
                self.sub_strategies.len().min(1)
            }
            Some(h) if !h.has_high_capability => {
                self.sub_strategies.len().min(2)
            }
            _ => self.sub_strategies.len(),
        }
    }

    fn select_model(&self, ctx: &CompilationContext) -> String {
        if let Some(hints) = &self.model_hints {
            if let Some(best) = hints.eligible_models.first() {
                return best.clone();
            }
        }
        ctx.available_models.first().cloned().unwrap_or_else(|| "default".into())
    }
}

impl Strategy for FusionStrategy {
    fn descriptor(&self) -> StrategyDescriptor {
        let expected = match self.model_hints.as_ref().map(|h| h.capability()) {
            Some(ModelCapability::High) => vec![ArtifactKind::Debate, ArtifactKind::Generic],
            _ => vec![ArtifactKind::Generic],
        };
        StrategyDescriptor {
            name: "Fusion".into(),
            parallelism: Parallelism::Unlimited,
            requires_barrier: true,
            supports_streaming: StreamingMode::IncrementalArtifacts,
            retry_policy: RetryPolicy {
                max_retries: 2,
                backoff_ms: 1000,
            },
            expected_outputs: expected,
        }
    }

    fn lower(&self, _ir: &StrategyIR, ctx: &CompilationContext) -> Result<PrimitiveGraph, CompilerDiagnostic> {
        let count = self.active_count() as u32;
        if count < 1 {
            return Err(CompilerDiagnostic::error(
                "E0103",
                "Fusion strategy requires at least 1 sub-strategy",
            ));
        }

        let mut graph = PrimitiveGraph::new("fusion_graph");
        let model = self.select_model(ctx);

        if count == 1 {
            graph.add_node(PrimitiveNode {
                id: "fusion_single".into(),
                kind: PrimitiveNodeKind::LLMGenerate { model, role: None },
                artifact_kind: Some("Generic".into()),
            });
            return Ok(graph);
        }

        let reducer_mode = match self.model_hints.as_ref().map(|h| h.capability()) {
            Some(ModelCapability::High) => ReducerMode::Debate,
            _ => ReducerMode::Merge,
        };

        graph.add_node(PrimitiveNode {
            id: "fanout_fusion".into(),
            kind: PrimitiveNodeKind::FanOut { count },
            artifact_kind: None,
        });

        for i in 0..count {
            let gen_id = format!("fusion_gen_{}", i + 1);
            graph.add_node(PrimitiveNode {
                id: gen_id.clone(),
                kind: PrimitiveNodeKind::LLMGenerate {
                    model: model.clone(),
                    role: Some(format!("fusion_member_{}", i + 1)),
                },
                artifact_kind: Some("Generic".into()),
            });
            graph.add_edge("fanout_fusion", gen_id.clone(), None);
            graph.add_edge(gen_id, "barrier_fusion", None);
        }

        graph.add_node(PrimitiveNode {
            id: "barrier_fusion".into(),
            kind: PrimitiveNodeKind::Barrier {
                min_completion: 1.0,
                timeout: Duration::from_secs(120),
                on_failure: BarrierFailurePolicy::Continue,
            },
            artifact_kind: None,
        });

        graph.add_node(PrimitiveNode {
            id: "reducer_fusion".into(),
            kind: PrimitiveNodeKind::Reducer {
                mode: reducer_mode,
                model,
            },
            artifact_kind: Some("Generic".into()),
        });

        graph.add_edge("barrier_fusion", "reducer_fusion", None);

        Ok(graph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_models_detects_high_capability() {
        let models: Vec<String> = vec!["gpt-4".into(), "claude-haiku".into()];
        let availability = ModelAvailability::from_models(&models);

        assert!(availability.has_high_capability);
        assert!(availability.has_medium_capability);
        assert_eq!(availability.eligible_models, models);
    }

    #[test]
    fn test_from_models_detects_medium_capability() {
        let availability = ModelAvailability::from_models(&["gpt-4o-mini".into()]);

        assert!(!availability.has_high_capability);
        assert!(availability.has_medium_capability);
        assert_eq!(availability.eligible_models, vec!["gpt-4o-mini".to_string()]);
    }

    #[test]
    fn test_from_models_no_known_keywords() {
        let availability = ModelAvailability::from_models(&["local-llm-7b".into()]);

        assert!(!availability.has_high_capability);
        assert!(!availability.has_medium_capability);
    }

    #[test]
    fn test_from_models_empty() {
        let availability = ModelAvailability::from_models(&[]);

        assert!(!availability.has_high_capability);
        assert!(!availability.has_medium_capability);
        assert!(availability.eligible_models.is_empty());
    }

    #[test]
    fn test_capability_high() {
        let availability = ModelAvailability {
            has_high_capability: true,
            has_medium_capability: true,
            eligible_models: vec!["gpt-4".into()],
        };

        assert!(matches!(availability.capability(), ModelCapability::High));
    }

    #[test]
    fn test_capability_medium() {
        let availability = ModelAvailability {
            has_high_capability: false,
            has_medium_capability: true,
            eligible_models: vec!["gpt-4o-mini".into()],
        };

        assert!(matches!(availability.capability(), ModelCapability::Medium));
    }

    #[test]
    fn test_capability_low() {
        let availability = ModelAvailability {
            has_high_capability: false,
            has_medium_capability: false,
            eligible_models: vec!["local-llm-7b".into()],
        };

        assert!(matches!(availability.capability(), ModelCapability::Low));
    }

    #[test]
    fn test_new_has_no_model_hints() {
        let strategy = FusionStrategy::new(Vec::new());

        assert!(strategy.model_hints.is_none());
        assert!(strategy.sub_strategies.is_empty());
    }

    #[test]
    fn test_matches_keyword_variants() {
        let availability = ModelAvailability::from_models(&[
            "gpt-4".into(),
            "gpt-4-turbo".into(),
            "gpt-4.5".into(),
            "my-gpt-4-custom".into(),
            "my-gpt-4.custom".into(),
        ]);
        assert!(availability.has_high_capability);
    }
}
