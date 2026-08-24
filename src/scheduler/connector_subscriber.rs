use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::config::error::ReloadError;
use crate::config::manager::{ConfigSubscriber, ConfigSnapshot};
use crate::config::ConnectorConfig;
use crate::scheduler::connector_resolver::{Connector, ConnectorResolver};

/// Subscriber that hot-swaps connectors on config reload.
pub struct ConnectorSubscriber {
    resolver: ConnectorResolver,
    candidates: RwLock<Option<HashMap<String, Arc<dyn Connector>>>>,
}

impl ConnectorSubscriber {
    pub fn new(resolver: ConnectorResolver) -> Self {
        Self {
            resolver,
            candidates: RwLock::new(None),
        }
    }
}

impl ConfigSubscriber for ConnectorSubscriber {
    fn priority(&self) -> u8 {
        5
    }

    fn prepare(
        &self,
        _old: &ConfigSnapshot,
        new: &ConfigSnapshot,
    ) -> Result<(), ReloadError> {
        let mut candidates = HashMap::new();

        for (name, cfg) in &new.config.connectors {
            let connector = create_connector(name, cfg)?;
            candidates.insert(name.clone(), connector);
        }

        *self.candidates.write() = Some(candidates);
        Ok(())
    }

    fn commit(&self, generation: u64) {
        let candidates = self.candidates.write().take();
        if let Some(candidates) = candidates {
            let old_names: Vec<String> = self.resolver.connector_names();
            let new_names: Vec<String> = candidates.keys().cloned().collect();

            let added: Vec<&String> =
                new_names.iter().filter(|n| !old_names.contains(n)).collect();
            let removed: Vec<&String> =
                old_names.iter().filter(|n| !new_names.contains(n)).collect();
            let updated: Vec<&String> =
                new_names.iter().filter(|n| old_names.contains(n)).collect();

            tracing::info!(
                generation,
                added = ?added,
                removed = ?removed,
                updated = ?updated,
                "ConnectorSubscriber commit"
            );

            self.resolver.clear();
            for (_, connector) in candidates {
                if let Err(e) = self.resolver.register_connector(connector) {
                    tracing::warn!("Skipping connector registration: {e}");
                }
            }
        }
    }
}

fn create_connector(
    name: &str,
    cfg: &ConnectorConfig,
) -> Result<Arc<dyn Connector>, ReloadError> {
    match cfg.connector_type.as_str() {
        "http" => Ok(Arc::new(crate::connectors::http::HttpConnector::new())),
        "shell" => Ok(Arc::new(crate::connectors::shell::ShellConnector::new())),
        "github" => Ok(Arc::new(crate::connectors::github::GitHubConnector::new())),
        "filesystem" => Ok(Arc::new(crate::connectors::filesystem::FilesystemConnector::new())),
        "browser" => Ok(Arc::new(crate::connectors::browser::BrowserConnector::new())),
        "mcp" => Ok(Arc::new(crate::connectors::mcp::McpConnector::new())),
        _ => Err(ReloadError::ConnectorError(format!(
            "Unknown connector type: {} for connector '{}'",
            cfg.connector_type, name
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    fn make_snapshot(generation: u64, connectors: HashMap<String, ConnectorConfig>) -> ConfigSnapshot {
        let config = AppConfig {
            unsafe_dev: false,
            server: crate::config::ServerConfig {
                host: "0.0.0.0".into(),
                port: 8080,
                shutdown_timeout_secs: 30,
                request_timeout_secs: 300,
                cors: crate::config::CorsConfig::default(),
            },
            resources: crate::config::ResourceConfig {
                max_daily_cost: crate::types::NanoUSD::from_nanos(100_000_000_000),
                max_daily_tokens: 1_000_000,
                max_concurrent: 5,
                max_concurrent_nodes: 16,
                provider_limits: HashMap::new(),
            },
            providers: HashMap::new(),
            policies: vec![],
            strategies: crate::config::StrategyConfig::default(),
            tools: crate::config::ToolsConfig::default(),
            auth: crate::config::AuthConfig::default(),
            rate_limiting: crate::config::RateLimitingConfig::default(),
            logging: crate::config::LoggingConfig::default(),
            model_catalog: crate::types::ModelCatalog::default(),
            connectors,
            features: HashMap::new(),
        };
        ConfigSnapshot {
            generation,
            config: Arc::new(config),
        }
    }

    #[test]
    fn test_prepare_valid_connectors() {
        let resolver = ConnectorResolver::new();
        let subscriber = ConnectorSubscriber::new(resolver.clone());

        let mut connectors = HashMap::new();
        connectors.insert(
            "my-http".to_string(),
            ConnectorConfig {
                connector_type: "http".to_string(),
                config: HashMap::new(),
            },
        );
        let old = make_snapshot(1, HashMap::new());
        let new = make_snapshot(2, connectors);

        assert!(subscriber.prepare(&old, &new).is_ok());
    }

    #[test]
    fn test_prepare_invalid_connector_type() {
        let resolver = ConnectorResolver::new();
        let subscriber = ConnectorSubscriber::new(resolver.clone());

        let mut connectors = HashMap::new();
        connectors.insert(
            "bad".to_string(),
            ConnectorConfig {
                connector_type: "nonexistent".to_string(),
                config: HashMap::new(),
            },
        );
        let old = make_snapshot(1, HashMap::new());
        let new = make_snapshot(2, connectors);

        let result = subscriber.prepare(&old, &new);
        assert!(result.is_err());
        match result.unwrap_err() {
            ReloadError::ConnectorError(msg) => {
                assert!(msg.contains("nonexistent"));
            }
            other => panic!("expected ConnectorError, got {other:?}"),
        }
    }

    #[test]
    fn test_commit_swaps_connectors() {
        let resolver = ConnectorResolver::new();
        let subscriber = ConnectorSubscriber::new(resolver.clone());

        let mut connectors = HashMap::new();
        connectors.insert(
            "my-http".to_string(),
            ConnectorConfig {
                connector_type: "http".to_string(),
                config: HashMap::new(),
            },
        );
        let old = make_snapshot(1, HashMap::new());
        let new = make_snapshot(2, connectors);

        subscriber.prepare(&old, &new).expect("prepare should succeed");
        subscriber.commit(2);

        let names = resolver.connector_names();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0], "http");
    }
}





