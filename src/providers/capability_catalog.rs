use std::collections::HashMap;
use std::sync::OnceLock;

use crate::config::{AppConfig, CapabilityDescriptor};
use super::{ModelCapabilities, ModelPricing, ModelRequirements};

/// Step-type to capability requirements mapping.
/// Declarative: add new step types here instead of scattering match arms.
fn step_requirements() -> &'static HashMap<&'static str, ModelRequirements> {
    static MAP: OnceLock<HashMap<&'static str, ModelRequirements>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("generate", ModelRequirements {
            min_reasoning_score: Some(0.5),
            ..Default::default()
        });
        m.insert("judge", ModelRequirements {
            min_reasoning_score: Some(0.8),
            ..Default::default()
        });
        m.insert("tool_calling", ModelRequirements {
            requires_tools: true,
            min_coding_score: Some(0.6),
            ..Default::default()
        });
        m.insert("review", ModelRequirements {
            min_reasoning_score: Some(0.7),
            ..Default::default()
        });
        m.insert("vision", ModelRequirements {
            requires_vision: true,
            ..Default::default()
        });
        m.insert("fast", ModelRequirements {
            max_cost_per_1k_tokens: Some(0.01),
            ..Default::default()
        });
        m.insert("cheap", ModelRequirements {
            max_cost_per_1k_tokens: Some(0.005),
            ..Default::default()
        });
        m
    })
}

#[derive(Debug, Clone)]
pub struct ModelCandidate {
    pub provider_name: String,
    pub model_id: String,
    pub descriptor: CapabilityDescriptor,
    pub capabilities: ModelCapabilities,
    pub pricing: ModelPricing,
}

#[derive(Debug, Clone, Default)]
pub struct CapabilityCatalog {
    entries: Vec<ModelCandidate>,
}

impl CapabilityCatalog {
    pub fn from_config(config: &AppConfig) -> Self {
        let mut entries = Vec::new();
        for (provider_name, provider_cfg) in &config.providers {
            for (model_id, desc) in &provider_cfg.models {
                let caps = descriptor_to_capabilities(desc);
                let pricing = descriptor_to_pricing(desc);
                entries.push(ModelCandidate {
                    provider_name: provider_name.clone(),
                    model_id: model_id.clone(),
                    descriptor: desc.clone(),
                    capabilities: caps,
                    pricing,
                });
            }
        }
        Self { entries }
    }

    pub fn resolve(&self, requirements: &ModelRequirements) -> Vec<ModelCandidate> {
        let mut matches: Vec<ModelCandidate> = self.entries.iter()
            .filter(|e| requirements.matches(&e.capabilities, &e.pricing))
            .cloned()
            .collect();

        matches.sort_by(|a, b| {
            let score_a = a.capabilities.coding_score + a.capabilities.reasoning_score;
            let score_b = b.capabilities.coding_score + b.capabilities.reasoning_score;
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        matches
    }

    pub fn all_entries(&self) -> &[ModelCandidate] {
        &self.entries
    }

    pub fn find_by_model(&self, model_id: &str) -> Option<&ModelCandidate> {
        self.entries.iter().find(|e| e.model_id == model_id)
    }

    pub fn find_by_provider(&self, provider_name: &str) -> Vec<&ModelCandidate> {
        self.entries.iter().filter(|e| e.provider_name == provider_name).collect()
    }

    /// Query providers that match a specific step type (e.g., "generate", "judge", "tool_calling").
    /// Returns candidates sorted by combined score, filtered by the step's requirements.
    /// If the step type is unknown, returns all entries sorted by score.
    pub fn query_by_capability(&self, step: &str) -> Vec<ModelCandidate> {
        let reqs = step_requirements()
            .get(step)
            .cloned()
            .unwrap_or_default();
        self.resolve(&reqs)
    }
}

fn descriptor_to_capabilities(desc: &CapabilityDescriptor) -> ModelCapabilities {
    ModelCapabilities {
        coding_score: desc.coding_score.unwrap_or(0.0),
        reasoning_score: desc.reasoning_score.unwrap_or(0.0),
        max_context_tokens: desc.context_limit.unwrap_or(0),
        max_output_tokens: desc.output_limit.unwrap_or(0),
        supports_tools: desc.supports_tools.unwrap_or(false),
        supports_streaming: desc.supports_streaming.unwrap_or(false),
        supports_vision: desc.supports_vision.unwrap_or(false),
        supports_audio: desc.supports_audio.unwrap_or(false),
        supports_pdf: desc.supports_pdf.unwrap_or(false),
        supports_json_mode: desc.supports_json_mode.unwrap_or(false),
        supports_thinking: desc.supports_thinking.unwrap_or(false),
        supports_parallel_tools: desc.supports_parallel_tools.unwrap_or(false),
        supports_structured_output: desc.supports_structured_output.unwrap_or(false),
    }
}

fn descriptor_to_pricing(desc: &CapabilityDescriptor) -> ModelPricing {
    ModelPricing {
        input_cost_per_1k: desc.input_cost_per_1k.unwrap_or(0.0),
        output_cost_per_1k: desc.output_cost_per_1k.unwrap_or(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use std::collections::HashMap;

    fn test_config() -> AppConfig {
        let mut providers = HashMap::new();
        let mut models = HashMap::new();
        models.insert("deepseek-chat".into(), CapabilityDescriptor {
            name: Some("DeepSeek Chat".into()),
            context_limit: Some(128_000),
            output_limit: Some(8_192),
            coding_score: Some(0.9),
            reasoning_score: Some(0.85),
            supports_tools: Some(true),
            supports_streaming: Some(true),
            supports_vision: Some(false),
            input_cost_per_1k: Some(0.00014),
            output_cost_per_1k: Some(0.00028),
            ..Default::default()
        });
        providers.insert("deepseek".into(), ProviderConfig {
            transport: "openai-chat".into(),
            base_url: Some("https://api.deepseek.com/v1".into()),
            models,
            ..Default::default()
        });
        let mut models2 = HashMap::new();
        models2.insert("gpt-4o".into(), CapabilityDescriptor {
            name: Some("GPT-4o".into()),
            context_limit: Some(128_000),
            output_limit: Some(16_384),
            coding_score: Some(0.92),
            reasoning_score: Some(0.88),
            supports_tools: Some(true),
            supports_streaming: Some(true),
            supports_vision: Some(true),
            input_cost_per_1k: Some(0.005),
            output_cost_per_1k: Some(0.015),
            ..Default::default()
        });
        providers.insert("openai".into(), ProviderConfig {
            transport: "openai-chat".into(),
            base_url: Some("https://api.openai.com/v1".into()),
            models: models2,
            ..Default::default()
        });
        AppConfig {
            unsafe_dev: false,
            server: ServerConfig {
                host: "127.0.0.1".into(),
                port: 8080,
                shutdown_timeout_secs: 30,
                cors: CorsConfig::default(),
            },
            resources: ResourceConfig {
                max_daily_cost: crate::types::NanoUSD::from_nanos(100_000_000_000),
                max_daily_tokens: 1_000_000,
                max_concurrent: 5,
                max_concurrent_nodes: 16,
                provider_limits: HashMap::new(),
            },
            policies: Vec::new(),
            providers,
            strategies: StrategyConfig::default(),
            tools: ToolsConfig::default(),
            auth: AuthConfig::default(),
            rate_limiting: RateLimitingConfig::default(),
            logging: LoggingConfig::default(),
            model_catalog: crate::types::ModelCatalog::default(),
            connectors: HashMap::new(),
            features: HashMap::new(),
        }
    }

    #[test]
    fn test_catalog_from_config() {
        let catalog = CapabilityCatalog::from_config(&test_config());
        assert_eq!(catalog.all_entries().len(), 2);
    }

    #[test]
    fn test_resolve_returns_matching_models() {
        let catalog = CapabilityCatalog::from_config(&test_config());
        let reqs = ModelRequirements {
            requires_tools: true,
            ..Default::default()
        };
        let candidates = catalog.resolve(&reqs);
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn test_resolve_filters_by_context_window() {
        let catalog = CapabilityCatalog::from_config(&test_config());
        let reqs = ModelRequirements {
            min_context_tokens: Some(200_000),
            ..Default::default()
        };
        let candidates = catalog.resolve(&reqs);
        assert_eq!(candidates.len(), 0);
    }

    #[test]
    fn test_resolve_filters_by_cost() {
        let catalog = CapabilityCatalog::from_config(&test_config());
        let reqs = ModelRequirements {
            max_cost_per_1k_tokens: Some(0.001),
            ..Default::default()
        };
        let candidates = catalog.resolve(&reqs);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].model_id, "deepseek-chat");
    }

    #[test]
    fn test_resolve_sorts_by_score() {
        let catalog = CapabilityCatalog::from_config(&test_config());
        let reqs = ModelRequirements::default();
        let candidates = catalog.resolve(&reqs);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].model_id, "gpt-4o");
        assert_eq!(candidates[1].model_id, "deepseek-chat");
    }

    #[test]
    fn test_find_by_model() {
        let catalog = CapabilityCatalog::from_config(&test_config());
        let found = catalog.find_by_model("deepseek-chat");
        assert!(found.is_some());
        assert_eq!(found.unwrap().provider_name, "deepseek");
    }

    #[test]
    fn test_find_by_provider() {
        let catalog = CapabilityCatalog::from_config(&test_config());
        let found = catalog.find_by_provider("deepseek");
        assert_eq!(found.len(), 1);
    }
}
