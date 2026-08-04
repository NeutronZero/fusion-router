use std::collections::HashMap;

use parking_lot::RwLock;

use crate::config::error::ReloadError;
use crate::config::manager::{ConfigSnapshot, ConfigSubscriber};
use crate::feature_gate::{FeatureConfig, FeatureDefinition, FeatureRegistry};

pub struct FeatureGateSubscriber {
    registry: RwLock<FeatureRegistry>,
    pending: RwLock<Option<HashMap<String, FeatureConfig>>>,
}

impl FeatureGateSubscriber {
    pub fn new(definitions: &'static [FeatureDefinition]) -> Self {
        Self {
            registry: RwLock::new(FeatureRegistry::new(definitions)),
            pending: RwLock::new(None),
        }
    }

    pub fn registry(&self) -> parking_lot::RwLockReadGuard<'_, FeatureRegistry> {
        self.registry.read()
    }

    #[allow(dead_code)]
    fn rollback(&self) {
        *self.pending.write() = None;
    }
}

impl ConfigSubscriber for FeatureGateSubscriber {
    fn prepare(&self, _old: &ConfigSnapshot, new: &ConfigSnapshot) -> Result<(), ReloadError> {
        let features = new.config.features.clone();
        *self.pending.write() = Some(features);
        Ok(())
    }

    fn commit(&self, _generation: u64) {
        if let Some(config) = self.pending.write().take() {
            self.registry.write().apply_config(&config);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::config::{
        AppConfig, AuthConfig, CorsConfig, LoggingConfig,
        RateLimitingConfig, ResourceConfig, ServerConfig, StrategyConfig, ToolsConfig,
    };
    use crate::config::manager::{ConfigSnapshot, ConfigSubscriber};
    use crate::feature_gate::config_subscriber::FeatureGateSubscriber;
    use crate::feature_gate::{FeatureConfig, FeatureDefinition, FeatureFlag, Stability};
    use crate::types::ModelCatalog;

    const TEST_DEFINITIONS: &[FeatureDefinition] = &[
        FeatureDefinition {
            id: FeatureFlag::Streaming,
            introduced: "0.1.0",
            removed: None,
            stability: Stability::Stable,
            default_enabled: true,
            description: "Enable streaming responses",
        },
    ];

    fn minimal_app_config() -> AppConfig {
        AppConfig {
            unsafe_dev: false,
            server: ServerConfig {
                host: "0.0.0.0".into(),
                port: 8080,
                shutdown_timeout_secs: 30,
                cors: CorsConfig::default(),
            },
            resources: ResourceConfig {
                max_daily_cost: 100.0,
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
        }
    }

    fn make_snapshot(features: HashMap<String, FeatureConfig>) -> ConfigSnapshot {
        ConfigSnapshot {
            generation: 1,
            config: Arc::new(AppConfig {
                features,
                ..minimal_app_config()
            }),
        }
    }

    #[test]
    fn test_subscriber_prepare_parses_overrides() {
        let subscriber = FeatureGateSubscriber::new(TEST_DEFINITIONS);
        let old = make_snapshot(HashMap::new());
        let new = make_snapshot(HashMap::new());

        let result = subscriber.prepare(&old, &new);

        assert!(result.is_ok());
        assert!(subscriber.pending.read().is_some());
    }

    #[test]
    fn test_subscriber_commit_applies_changes() {
        let subscriber = FeatureGateSubscriber::new(TEST_DEFINITIONS);

        assert!(subscriber.registry().is_enabled(FeatureFlag::Streaming));

        let old = make_snapshot(HashMap::new());
        let mut new_features = HashMap::new();
        new_features.insert("streaming".into(), FeatureConfig { enabled: false });
        let new = make_snapshot(new_features);

        subscriber.prepare(&old, &new).unwrap();
        subscriber.commit(2);

        assert!(!subscriber.registry().is_enabled(FeatureFlag::Streaming));
    }

    #[test]
    fn test_subscriber_rollback_discards() {
        let subscriber = FeatureGateSubscriber::new(TEST_DEFINITIONS);

        assert!(subscriber.registry().is_enabled(FeatureFlag::Streaming));

        let old = make_snapshot(HashMap::new());
        let mut new_features = HashMap::new();
        new_features.insert("streaming".into(), FeatureConfig { enabled: false });
        let new = make_snapshot(new_features);

        subscriber.prepare(&old, &new).unwrap();
        subscriber.rollback();

        assert!(subscriber.registry().is_enabled(FeatureFlag::Streaming));
        assert!(subscriber.pending.read().is_none());
    }
}
