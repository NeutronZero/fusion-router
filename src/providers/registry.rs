use super::router::ProviderTarget;
use super::ChatProvider;
use crate::config::error::ReloadError;
use crate::config::manager::{ConfigSnapshot, ConfigSubscriber};
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ChatStreamChunk};
use async_trait::async_trait;
use futures::stream::BoxStream;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ProviderRegistryConfig {
    pub targets: HashMap<String, Vec<String>>, // prefix -> target_names
}

pub struct ProviderRegistry {
    targets: parking_lot::RwLock<HashMap<String, Arc<ProviderTarget>>>,
    prefixes: parking_lot::RwLock<Vec<(Vec<String>, Arc<ProviderTarget>)>>,
    capabilities: parking_lot::RwLock<HashMap<String, super::ModelCapabilities>>,
    pricing: parking_lot::RwLock<HashMap<String, super::ModelPricing>>,
    default_target: Arc<ProviderTarget>,
    version: Arc<AtomicU64>,
    candidates: parking_lot::RwLock<Option<HashMap<String, Arc<ProviderTarget>>>>,
}

impl ProviderRegistry {
    pub fn new(default_target: ProviderTarget) -> Self {
        Self {
            targets: parking_lot::RwLock::new(HashMap::new()),
            prefixes: parking_lot::RwLock::new(Vec::new()),
            capabilities: parking_lot::RwLock::new(HashMap::new()),
            pricing: parking_lot::RwLock::new(HashMap::new()),
            default_target: Arc::new(default_target),
            version: Arc::new(AtomicU64::new(0)),
            candidates: parking_lot::RwLock::new(None),
        }
    }

    pub fn register_target(&self, prefixes: Vec<String>, target: ProviderTarget) {
        let target_arc = Arc::new(target);
        // Both mappings are mutated under a single critical section so a
        // concurrent `get_matching_targets` (which reads only `prefixes`)
        // can never observe a prefix dangling to a not-yet-registered
        // target (or vice versa during removal).
        {
            let mut targets = self.targets.write();
            let mut prefix_list = self.prefixes.write();
            targets.insert(target_arc.name.clone(), target_arc.clone());
            prefix_list.push((prefixes, target_arc));
        }
        self.version.fetch_add(1, Ordering::Release);
    }

    pub fn register_target_with_capabilities(
        &self,
        prefixes: Vec<String>,
        target: ProviderTarget,
        caps: super::ModelCapabilities,
        pricing: super::ModelPricing,
    ) {
        let name = target.name.clone();
        self.register_target(prefixes, target);
        self.capabilities.write().insert(name.clone(), caps);
        self.pricing.write().insert(name, pricing);
    }

    pub fn get_capabilities(&self, name: &str) -> Option<super::ModelCapabilities> {
        self.capabilities.read().get(name).cloned()
    }

    pub fn get_pricing(&self, name: &str) -> Option<super::ModelPricing> {
        self.pricing.read().get(name).cloned()
    }

    /// Number of registered provider targets (default included).
    pub fn target_count(&self) -> usize {
        self.targets.read().len()
    }

    pub fn update_capabilities(&self, name: &str, caps: super::ModelCapabilities) {
        let mut map = self.capabilities.write();
        if map.contains_key(name) {
            map.insert(name.to_string(), caps);
            self.version.fetch_add(1, Ordering::Release);
        }
    }

    pub fn unregister_target(&self, name: &str) -> bool {
        let removed = {
            let mut targets = self.targets.write();
            let mut prefix_list = self.prefixes.write();
            let removed = targets.remove(name).is_some();
            if removed {
                prefix_list.retain(|(_, target)| target.name != name);
            }
            removed
        };
        if removed {
            self.capabilities.write().remove(name);
            self.pricing.write().remove(name);
            self.version.fetch_add(1, Ordering::Release);
        }
        removed
    }

    pub fn get_matching_targets(&self, model: &str) -> Vec<Arc<ProviderTarget>> {
        let prefix_list = self.prefixes.read();
        let mut matched = Vec::new();
        for (prefixes, target) in prefix_list.iter() {
            for prefix in prefixes {
                if model.starts_with(prefix) {
                    matched.push(target.clone());
                    break;
                }
            }
        }
        if matched.is_empty() {
            matched.push(self.default_target.clone());
        }
        matched
    }

    pub fn select_targets(&self, reqs: &super::ModelRequirements) -> Vec<Arc<ProviderTarget>> {
        let targets = self.targets.read();
        let caps_map = self.capabilities.read();
        let pricing_map = self.pricing.read();

        let mut candidates: Vec<Arc<ProviderTarget>> = targets
            .values()
            .filter(|t| t.can_execute())
            .filter(|t| {
                caps_map
                    .get(&t.name)
                    .zip(pricing_map.get(&t.name))
                    .map(|(c, p)| reqs.matches(c, p))
                    .unwrap_or(true)
            })
            .cloned()
            .collect();

        candidates.sort_by(|a, b| {
            let cost_a = pricing_map
                .get(&a.name)
                .map(|p| p.input_cost_per_1k + p.output_cost_per_1k)
                .unwrap_or(crate::types::NanoUSD::from_nanos(u64::MAX));
            let cost_b = pricing_map
                .get(&b.name)
                .map(|p| p.input_cost_per_1k + p.output_cost_per_1k)
                .unwrap_or(crate::types::NanoUSD::from_nanos(u64::MAX));
            cost_a
                .partial_cmp(&cost_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates
    }

    pub fn version(&self) -> Arc<AtomicU64> {
        self.version.clone()
    }
}

#[async_trait]
impl ChatProvider for ProviderRegistry {
    fn name(&self) -> &str {
        "provider-registry"
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<ChatCompletionResponse> {
        let targets = self.get_matching_targets(&request.model);
        let mut last_err: Option<anyhow::Error> = None;
        for target in &targets {
            if !target.can_execute() {
                tracing::warn!(provider = %target.name, "circuit open, skipping");
                continue;
            }
            let provider = target.get_or_init().await?;
            match provider.chat_completion(request).await {
                Ok(resp) => {
                    target.record_success();
                    return Ok(resp);
                }
                Err(e) => {
                    tracing::warn!(provider = %target.name, error = %e, "provider failed, trying next");
                    target.record_failure();
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            anyhow::anyhow!("no available providers for model: {}", request.model)
        }))
    }

    async fn chat_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<ChatStreamChunk>>> {
        let targets = self.get_matching_targets(&request.model);
        let mut last_err: Option<anyhow::Error> = None;
        for target in &targets {
            if !target.can_execute() {
                tracing::warn!(provider = %target.name, "circuit open, skipping");
                continue;
            }
            let provider = target.get_or_init().await?;
            match provider.chat_stream(request).await {
                Ok(stream) => {
                    target.record_success();
                    return Ok(stream);
                }
                Err(e) => {
                    tracing::warn!(provider = %target.name, error = %e, "provider stream failed, trying next");
                    target.record_failure();
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            anyhow::anyhow!("no available providers for model: {}", request.model)
        }))
    }
}

impl ConfigSubscriber for Arc<ProviderRegistry> {
    fn priority(&self) -> u8 {
        (**self).priority()
    }

    fn prepare(&self, old: &ConfigSnapshot, new: &ConfigSnapshot) -> Result<(), ReloadError> {
        (**self).prepare(old, new)
    }

    fn commit(&self, generation: u64) {
        (**self).commit(generation)
    }
}

impl ConfigSubscriber for ProviderRegistry {
    fn priority(&self) -> u8 {
        10
    }

    fn prepare(&self, _old: &ConfigSnapshot, new: &ConfigSnapshot) -> Result<(), ReloadError> {
        use super::factory;

        let mut candidates = HashMap::new();

        for (name, cfg) in &new.config.providers {
            let api_key = factory::resolve_api_key(cfg, name, false).map_err(|e| {
                ReloadError::Subscriber {
                    name: "ProviderRegistry".into(),
                    reason: e.to_string(),
                }
            })?;

            let target = factory::create_reload_target(name, cfg, api_key);
            candidates.insert(name.clone(), Arc::new(target));
        }

        *self.candidates.write() = Some(candidates);
        Ok(())
    }

    fn commit(&self, generation: u64) {
        let candidates = self.candidates.write().take();
        if let Some(candidates) = candidates {
            let mut targets = self.targets.write();
            let old_names: Vec<String> = targets.keys().cloned().collect();
            let new_names: Vec<String> = candidates.keys().cloned().collect();

            let added: Vec<&String> = new_names
                .iter()
                .filter(|n| !old_names.contains(n))
                .collect();
            let removed: Vec<&String> = old_names
                .iter()
                .filter(|n| !new_names.contains(n))
                .collect();
            let updated: Vec<&String> =
                new_names.iter().filter(|n| old_names.contains(n)).collect();

            tracing::info!(
                generation,
                added = ?added,
                removed = ?removed,
                updated = ?updated,
                "ProviderRegistry commit"
            );

            let mut caps = self.capabilities.write();
            let mut pricing = self.pricing.write();
            for name in &removed {
                caps.remove(name.as_str());
                pricing.remove(name.as_str());
            }
            // Capability/pricing entries for providers that remain configured
            // are preserved by name, so capability filtering stays active
            // across config reloads.
            drop(caps);
            drop(pricing);

            // Prefixes and targets are replaced under a single critical
            // section so readers never observe a prefix-to-target mismatch
            // during the transition.
            let mut prefix_list = self.prefixes.write();
            prefix_list.clear();
            for (name, target) in &candidates {
                prefix_list.push((vec![name.clone() + "/"], target.clone()));
            }
            drop(prefix_list);

            *targets = candidates;
            // Bump the version so subscribers watching `version()` (e.g. the
            // router dashboard) observe the full target replacement.
            self.version.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::circuit_breaker::CircuitBreaker;
    use super::super::{ChatProvider, ModelCapabilities, ModelPricing, ModelRequirements};
    use super::*;
    use crate::types::NanoUSD;
    use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct DummyProvider;
    #[async_trait]
    impl ChatProvider for DummyProvider {
        fn name(&self) -> &str {
            "dummy"
        }
        async fn chat_completion(
            &self,
            _req: &ChatCompletionRequest,
        ) -> anyhow::Result<ChatCompletionResponse> {
            Ok(ChatCompletionResponse {
                id: "dummy".into(),
                object: "chat.completion".into(),
                created: 0,
                model: "dummy".into(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "dummy".into(),
                    },
                    finish_reason: "stop".into(),
                }],
                native_tool_calls: None,
                usage: None,
            })
        }
    }

    fn dummy_target(name: &str) -> ProviderTarget {
        ProviderTarget::new(
            name.into(),
            CircuitBreaker::new(3, 2, 5),
            Box::new(|| Arc::new(DummyProvider)),
        )
    }

    fn cheap_caps() -> ModelCapabilities {
        ModelCapabilities {
            coding_score: 0.5,
            reasoning_score: 0.5,
            max_context_tokens: 32_000,
            max_output_tokens: 0,
            supports_tools: false,
            supports_streaming: true,
            supports_vision: false,
            supports_audio: false,
            supports_pdf: false,
            supports_json_mode: true,
            supports_thinking: false,
            supports_parallel_tools: false,
            supports_structured_output: false,
        }
    }

    fn premium_caps() -> ModelCapabilities {
        ModelCapabilities {
            coding_score: 0.95,
            reasoning_score: 0.95,
            max_context_tokens: 200_000,
            max_output_tokens: 0,
            supports_tools: true,
            supports_streaming: true,
            supports_vision: true,
            supports_audio: false,
            supports_pdf: false,
            supports_json_mode: true,
            supports_thinking: false,
            supports_parallel_tools: false,
            supports_structured_output: false,
        }
    }

    fn cheap_pricing() -> ModelPricing {
        ModelPricing {
            input_cost_per_1k: NanoUSD::from_nanos(150_000_000),
            output_cost_per_1k: NanoUSD::from_nanos(600_000_000),
        }
    }

    fn premium_pricing() -> ModelPricing {
        ModelPricing {
            input_cost_per_1k: NanoUSD::from_nanos(10_000_000_000),
            output_cost_per_1k: NanoUSD::from_nanos(30_000_000_000),
        }
    }

    #[test]
    fn test_select_target_by_capability() {
        let registry = ProviderRegistry::new(dummy_target("default"));
        registry.register_target_with_capabilities(
            vec!["cheap/".into()],
            dummy_target("cheap-model"),
            cheap_caps(),
            cheap_pricing(),
        );
        registry.register_target_with_capabilities(
            vec!["premium/".into()],
            dummy_target("premium-model"),
            premium_caps(),
            premium_pricing(),
        );

        let req = ModelRequirements {
            requires_tools: true,
            requires_vision: true,
            ..Default::default()
        };
        let matching = registry.select_targets(&req);
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].name, "premium-model");
    }

    #[test]
    fn test_select_target_cost_sorting() {
        let registry = ProviderRegistry::new(dummy_target("default"));
        registry.register_target_with_capabilities(
            vec!["cheap/".into()],
            dummy_target("cheap-model"),
            cheap_caps(),
            cheap_pricing(),
        );
        registry.register_target_with_capabilities(
            vec!["premium/".into()],
            dummy_target("premium-model"),
            premium_caps(),
            premium_pricing(),
        );

        let req = ModelRequirements {
            requires_streaming: true,
            ..Default::default()
        };
        let matching = registry.select_targets(&req);
        assert_eq!(matching.len(), 2);
        assert_eq!(matching[0].name, "cheap-model");
        assert_eq!(matching[1].name, "premium-model");
    }

    #[test]
    fn test_select_target_circuit_breaker_filtering() {
        let registry = ProviderRegistry::new(dummy_target("default"));
        let broken_breaker = CircuitBreaker::new(1, 2, 60);
        broken_breaker.record_failure();
        let broken_target = ProviderTarget::new(
            "broken".into(),
            broken_breaker,
            Box::new(|| Arc::new(DummyProvider)),
        );
        registry.register_target_with_capabilities(
            vec!["broken/".into()],
            broken_target,
            cheap_caps(),
            cheap_pricing(),
        );
        registry.register_target_with_capabilities(
            vec!["good/".into()],
            dummy_target("good-model"),
            cheap_caps(),
            cheap_pricing(),
        );

        let matching = registry.select_targets(&ModelRequirements::default());
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].name, "good-model");
    }

    #[test]
    fn test_select_target_no_matches_returns_empty() {
        let registry = ProviderRegistry::new(dummy_target("default"));
        registry.register_target_with_capabilities(
            vec!["basic/".into()],
            dummy_target("basic-model"),
            cheap_caps(),
            cheap_pricing(),
        );

        let req = ModelRequirements {
            max_cost_per_1k_tokens: Some(NanoUSD::from_nanos(10_000_000)),
            ..Default::default()
        };
        let matching = registry.select_targets(&req);
        assert!(matching.is_empty());
    }
}
