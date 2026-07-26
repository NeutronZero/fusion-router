//! Phase 3A — `Connector`, `ConnectorDescriptor`, `BoundConnector`, & `ConnectorResolver` (`src/scheduler/connector_resolver.rs`)
//!
//! Late binding of abstract `CapabilityInstance` handles to concrete `Connector` instances at execution time.

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use fusion_plugin_api::{CapabilityId, CapabilityInstance, CapabilityExecutor};

/// Metadata descriptor exposing connector capabilities and configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConnectorDescriptor {
    pub name: String,
    pub version: semver::Version,
    pub supported_capabilities: Vec<CapabilityId>,
}

/// Core trait implemented by external system connectors (e.g. GitHub, Shell, Browser, Echo).
pub trait Connector: Send + Sync {
    fn descriptor(&self) -> ConnectorDescriptor;
    fn executor(&self) -> Arc<dyn CapabilityExecutor>;
}

/// Bound runtime handle pairing a `CapabilityInstance` with a concrete `Connector`.
#[derive(Clone)]
pub struct BoundConnector {
    pub instance: CapabilityInstance,
    pub connector_descriptor: ConnectorDescriptor,
    pub executor: Arc<dyn CapabilityExecutor>,
}

/// Thread-safe registry providing late-binding resolution of capabilities to connectors.
#[derive(Clone)]
pub struct ConnectorResolver {
    connectors: Arc<RwLock<HashMap<String, Arc<dyn Connector>>>>,
    capability_map: Arc<RwLock<HashMap<CapabilityId, String>>>,
}

impl ConnectorResolver {
    pub fn new() -> Self {
        Self {
            connectors: Arc::new(RwLock::new(HashMap::new())),
            capability_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Registers a concrete connector and maps its supported capabilities.
    pub fn register_connector(&self, connector: Arc<dyn Connector>) {
        let desc = connector.descriptor();
        let mut connectors_guard = self.connectors.write();
        let mut map_guard = self.capability_map.write();

        for cap_id in &desc.supported_capabilities {
            map_guard.insert(cap_id.clone(), desc.name.clone());
        }
        connectors_guard.insert(desc.name.clone(), connector);
    }

    /// Late-binds an abstract `CapabilityInstance` to a concrete `BoundConnector`.
    pub fn bind(&self, instance: &CapabilityInstance) -> Result<BoundConnector, String> {
        let map_guard = self.capability_map.read();
        let connector_name = map_guard.get(&instance.contract.id).ok_or_else(|| {
            format!("No connector registered for capability: {}", instance.contract.id)
        })?;

        let connectors_guard = self.connectors.read();
        let connector = connectors_guard.get(connector_name).ok_or_else(|| {
            format!("Connector '{}' registered for capability '{}' not found", connector_name, instance.contract.id)
        })?;

        Ok(BoundConnector {
            instance: instance.clone(),
            connector_descriptor: connector.descriptor(),
            executor: connector.executor(),
        })
    }
}

impl Default for ConnectorResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_plugin_api::{CapabilityContract, CapabilityId, CapabilityInstance};
    use fusion_plugin_echo::EchoPlugin;

    struct EchoConnector {
        plugin: Arc<EchoPlugin>,
    }

    impl Connector for EchoConnector {
        fn descriptor(&self) -> ConnectorDescriptor {
            ConnectorDescriptor {
                name: "echo".into(),
                version: semver::Version::parse("0.1.0").unwrap(),
                supported_capabilities: vec![
                    CapabilityId::new("echo.text"),
                    CapabilityId::new("echo.uppercase"),
                ],
            }
        }

        fn executor(&self) -> Arc<dyn CapabilityExecutor> {
            self.plugin.clone()
        }
    }

    #[test]
    fn test_connector_resolver_bind_success() {
        let resolver = ConnectorResolver::new();
        let connector = Arc::new(EchoConnector {
            plugin: Arc::new(EchoPlugin::new()),
        });

        resolver.register_connector(connector);

        let instance = fusion_plugin_api::CapabilityInstance {
            contract: fusion_plugin_api::CapabilityContract {
                id: fusion_plugin_api::CapabilityId::new("echo.text"),
                version: semver::Version::parse("0.1.0").unwrap(),
                description: "Echo".into(),
                inputs_schema: serde_json::json!({}),
                outputs_schema: serde_json::json!({}),
                permissions: vec![],
                estimated_cost_usd: 0.0,
                estimated_latency_ms: 1,
                reliability_score: 1.0,
                supports_streaming: false,
            },
            runtime_params: serde_json::json!({}),
        };

        let bound = resolver.bind(&instance).unwrap();
        assert_eq!(bound.connector_descriptor.name, "echo");
    }
}
