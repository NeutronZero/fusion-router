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
                self.resolver.register_connector(connector);
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
