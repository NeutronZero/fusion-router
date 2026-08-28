use super::router::ProviderTarget;
use super::ChatProvider;
use crate::config::error::ReloadError;
use crate::config::manager::{ConfigSnapshot, ConfigSubscriber};
use crate::types::{ChatCompletionRequest, ChatCompletionResponse, ChatStreamChunk, RouterError};
use async_trait::async_trait;
use futures::stream::BoxStream;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ProviderRegistryConfig {
    pub targets: HashMap<String, Vec<String>>, // prefix -> target_names
}

/// Deterministic default provider selection: the lexicographically smallest
/// configured provider name. `HashMap::keys().next()` is arbitrary hasher
/// order and made default routing depend on process seeds.
pub fn select_default_provider_name(names: &[String]) -> Option<String> {
    let mut sorted = names.to_vec();
    sorted.sort();
    sorted.into_iter().next()
}

pub struct ProviderRegistry {
    targets: parking_lot::RwLock<HashMap<String, Arc<ProviderTarget>>>,
    prefixes: parking_lot::RwLock<Vec<(Vec<String>, Arc<ProviderTarget>)>>,
    capabilities: parking_lot::RwLock<HashMap<String, super::ModelCapabilities>>,
    pricing: parking_lot::RwLock<HashMap<String, super::ModelPricing>>,
    default_target: parking_lot::RwLock<Arc<ProviderTarget>>,
    /// Default target rebuilt by `prepare()` from the incoming candidate set,
    /// swapped in by `commit()` so hot reload never leaves a stale default.
    pending_default: parking_lot::RwLock<Option<Arc<ProviderTarget>>>,
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
            default_target: parking_lot::RwLock::new(Arc::new(default_target)),
            pending_default: parking_lot::RwLock::new(None),
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

    /// Name of the current default (fallback) target.
    pub fn default_target_name(&self) -> String {
        self.default_target.read().name.clone()
    }

    /// Sorted, de-duplicated model prefixes currently registered.
    pub fn registered_prefixes(&self) -> Vec<String> {
        let mut prefixes: Vec<String> = self
            .prefixes
            .read()
            .iter()
            .flat_map(|(prefixes, _)| prefixes.iter().cloned())
            .collect();
        prefixes.sort();
        prefixes.dedup();
        prefixes
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

    /// Resolves the targets that may serve `model`.
    ///
    /// Fail-closed for multi-provider setups: when no configured prefix
    /// matches AND more than one provider family is registered, this returns
    /// EMPTY and the caller converts it to a typed no-route error — an
    /// unprefixed model must never silently ride whichever provider happened
    /// to be picked as default. Single-provider setups keep the default
    /// fallback (the default IS the only provider).
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
        if matched.is_empty() && prefix_list.len() <= 1 {
            matched.push(self.default_target.read().clone());
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
        if targets.is_empty() {
            return Err(Self::no_route_error(
                &request.model,
                self.registered_prefixes(),
            ));
        }
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
        if targets.is_empty() {
            return Err(Self::no_route_error(
                &request.model,
                self.registered_prefixes(),
            ));
        }
        let mut last_err: Option<anyhow::Error> = None;
        for target in &targets {
            if !target.can_execute() {
                tracing::warn!(provider = %target.name, "circuit open, skipping");
                continue;
            }
            let provider = target.get_or_init().await?;
            match provider.chat_stream(request).await {
                Ok(stream) => {
                    // Review L8: failures that arrive MID-STREAM previously
                    // never touched the circuit breaker because the connect
                    // path had already returned Ok. Observe the first item
                    // error so repeated upstream stream failures open the
                    // circuit like any other provider failure.
                    use futures::StreamExt;
                    let target_for_errors = target.clone();
                    let mut seen_error = false;
                    let monitored = stream.map(move |item| {
                        if !seen_error
                            && item.is_err() {
                                seen_error = true;
                                target_for_errors.record_failure();
                            }
                        item
                    });
                    target.record_success();
                    return Ok(Box::pin(monitored) as BoxStream<'static, _>);
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

impl ProviderRegistry {
    fn no_route_error(model: &str, available_prefixes: Vec<String>) -> anyhow::Error {
        let err = RouterError::NoRouteForModel {
            model: model.to_string(),
            available_prefixes,
        };
        tracing::warn!(error = %err, "routing rejected: no configured provider prefix matches");
        anyhow::Error::new(err)
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

        // Rebuild the default target from the SAME resolved candidate set
        // (same config + same resolve_api_key path as the targets themselves)
        // so hot reload cannot leave a stale default behind.
        let default_name =
            select_default_provider_name(&new.config.providers.keys().cloned().collect::<Vec<_>>());
        let pending_default = default_name.and_then(|name| candidates.get(&name).cloned());
        *self.pending_default.write() = pending_default;

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

            // Swap in the rebuilt default target. If the new configuration
            // has no providers at all there is nothing to rebuild from — keep
            // the previous default and say so loudly (it can only be reached
            // via the single-provider fallback path anyway).
            match self.pending_default.write().take() {
                Some(new_default) => {
                    let name = new_default.name.clone();
                    *self.default_target.write() = new_default;
                    tracing::info!(
                        generation,
                        default_provider = %name,
                        "default provider rebuilt after reload (lexicographically smallest configured provider)"
                    );
                }
                None => {
                    tracing::warn!(
                        generation,
                        current_default = %self.default_target.read().name,
                        "reload produced no providers; retaining previous default target"
                    );
                }
            }

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

    // -- fail-closed routing -------------------------------------------------

    use crate::config::manager::ConfigSnapshot;
    use crate::config::{
        AppConfig, AuthConfig, CorsConfig, LoggingConfig, RateLimitingConfig, ResourceConfig,
        ServerConfig, StrategyConfig, ToolsConfig,
    };
    use crate::types::ModelCatalog;

    fn empty_app_config() -> AppConfig {
        AppConfig {
            unsafe_dev: false,
            server: ServerConfig {
                host: "0.0.0.0".into(),
                port: 8080,
                shutdown_timeout_secs: 30,
                request_timeout_secs: 300,
                cors: CorsConfig::default(),
            },
            resources: ResourceConfig {
                max_daily_cost: NanoUSD::from_nanos(100_000_000_000),
                max_daily_tokens: 1_000_000,
                max_concurrent: 5,
                max_concurrent_nodes: 16,
                provider_limits: HashMap::new(),
            },
            policies: Vec::new(),
            providers: HashMap::new(),
            strategies: StrategyConfig::default(),
            tools: ToolsConfig::default(),
            auth: AuthConfig::default(),
            rate_limiting: RateLimitingConfig::default(),
            logging: LoggingConfig::default(),
            model_catalog: ModelCatalog::default(),
            connectors: HashMap::new(),
            features: HashMap::new(),
            streaming: Default::default(),
        }
    }

    fn snapshot_with_direct_key_providers(specs: &[(&str, &str)]) -> ConfigSnapshot {
        let mut config = empty_app_config();
        for (name, key) in specs {
            let mut cfg = crate::config::ProviderConfig::default();
            cfg.api_key = Some(key.to_string());
            config.providers.insert(name.to_string(), cfg);
        }
        ConfigSnapshot {
            generation: 2,
            config: Arc::new(config),
        }
    }

    fn unmatched_request(model: &str) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: model.into(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            files: None,
            execution: None,
            output: None,
            strategy: None,
        }
    }

    #[test]
    fn test_multi_provider_unmatched_model_returns_empty() {
        let registry = ProviderRegistry::new(dummy_target("default"));
        registry.register_target(vec!["zen/".into()], dummy_target("zen-target"));
        registry.register_target(vec!["openrouter/".into()], dummy_target("or-target"));

        assert!(
            registry.get_matching_targets("unknown/model").is_empty(),
            "unprefixed models must NOT silently route to default in multi-provider setups"
        );
        assert_eq!(registry.get_matching_targets("zen/x").len(), 1);
        assert_eq!(registry.get_matching_targets("openrouter/y").len(), 1);
    }

    #[test]
    fn test_single_provider_unmatched_model_falls_back_to_default() {
        let registry = ProviderRegistry::new(dummy_target("fallback"));
        registry.register_target(vec!["zen/".into()], dummy_target("zen-target"));

        let matched = registry.get_matching_targets("anything/else");
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].name, "fallback");
    }

    #[tokio::test]
    async fn test_chat_completion_no_route_is_typed_fail_closed_error() {
        let registry = ProviderRegistry::new(dummy_target("default"));
        registry.register_target(vec!["zen/".into()], dummy_target("zen-a"));
        registry.register_target(vec!["openrouter/".into()], dummy_target("or-a"));

        let err = registry
            .chat_completion(&unmatched_request("gpt-4o"))
            .await
            .expect_err("multi-provider unmatched model must fail closed");

        let router_err = err
            .downcast_ref::<RouterError>()
            .expect("typed RouterError");
        match router_err {
            RouterError::NoRouteForModel {
                model,
                available_prefixes,
            } => {
                assert_eq!(model, "gpt-4o");
                assert!(available_prefixes.contains(&"zen/".to_string()));
                assert!(available_prefixes.contains(&"openrouter/".to_string()));
            }
            other => panic!("expected NoRouteForModel, got {other:?}"),
        }

        let msg = router_err.user_message();
        assert!(msg.contains("zen/"), "client message must list prefixes");
        assert!(
            !msg.contains("sk-"),
            "message must not leak credential material"
        );
    }

    #[test]
    fn test_select_default_provider_name_deterministic() {
        for order in [
            vec!["zeta", "alpha", "midway"],
            vec!["midway", "zeta", "alpha"],
            vec!["alpha", "zeta", "midway"],
        ] {
            let names: Vec<String> = order.into_iter().map(String::from).collect();
            assert_eq!(
                select_default_provider_name(&names).as_deref(),
                Some("alpha"),
                "selection must be lexicographically smallest regardless of input order"
            );
        }
        assert_eq!(select_default_provider_name(&[]), None);
    }

    #[test]
    fn test_commit_rebuilds_default_target_from_new_candidates() {
        let registry = ProviderRegistry::new(dummy_target("startup-default"));

        // Two providers; HashMap iteration order is arbitrary, so the choice
        // of default after commit must be deterministic ("beta" < "gamma").
        let snapshot =
            snapshot_with_direct_key_providers(&[("gamma", "sk-gamma"), ("beta", "sk-beta")]);
        registry.prepare(&snapshot, &snapshot).expect("prepare ok");
        registry.commit(snapshot.generation);

        assert_eq!(
            registry.default_target_name(),
            "beta",
            "commit must rebuild the default from the new candidate set (lexicographically smallest)"
        );
        // Prefixes were swapped to the new candidate set.
        assert_eq!(registry.get_matching_targets("gamma/m")[0].name, "gamma");
    }

    #[test]
    fn test_commit_keeps_previous_default_when_reload_removes_all_providers() {
        let registry = ProviderRegistry::new(dummy_target("startup-default"));

        let with_provider = snapshot_with_direct_key_providers(&[("solo", "sk-solo")]);
        registry
            .prepare(&with_provider, &with_provider)
            .expect("prepare ok");
        registry.commit(with_provider.generation);

        let empty_snapshot = snapshot_with_direct_key_providers(&[]);
        registry
            .prepare(&with_provider, &empty_snapshot)
            .expect("prepare ok");
        registry.commit(empty_snapshot.generation);

        assert_eq!(
            registry.default_target_name(),
            "solo",
            "empty reload must retain the most recent valid default rather than regress"
        );
    }
}
