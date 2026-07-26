//! Phase 3A — `Connector`, `ConnectorDescriptor`, `BoundConnector`, & `ConnectorResolver` (`src/scheduler/connector_resolver.rs`)
//!
//! Late binding of abstract `CapabilityInstance` handles to concrete `Connector` instances at execution time.

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use fusion_plugin_api::{CapabilityId, CapabilityInstance, CapabilityExecutor};

/// The minimum connector version accepted during registration.
const MIN_SUPPORTED_RUNTIME_VERSION: semver::Version = semver::Version::new(0, 10, 0);

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
    pub(crate) connectors: Arc<RwLock<HashMap<String, Arc<dyn Connector>>>>,
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
    /// Returns an error if the connector version is below `MIN_SUPPORTED_RUNTIME_VERSION`.
    pub fn register_connector(&self, connector: Arc<dyn Connector>) -> Result<(), String> {
        let desc = connector.descriptor();

        if desc.version < MIN_SUPPORTED_RUNTIME_VERSION {
            return Err(format!(
                "connector version {} is below minimum supported {}",
                desc.version, MIN_SUPPORTED_RUNTIME_VERSION
            ));
        }

        let mut connectors_guard = self.connectors.write();
        let mut map_guard = self.capability_map.write();

        for cap_id in &desc.supported_capabilities {
            map_guard.insert(cap_id.clone(), desc.name.clone());
        }
        connectors_guard.insert(desc.name.clone(), connector);
        Ok(())
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

    /// Returns the names of all registered connectors.
    pub fn connector_names(&self) -> Vec<String> {
        self.connectors.read().keys().cloned().collect()
    }

    /// Remove a connector by name. Returns true if the connector existed.
    pub fn unregister_connector(&self, name: &str) -> bool {
        let mut connectors = self.connectors.write();
        let removed = connectors.remove(name).is_some();
        if removed {
            let mut cap_map = self.capability_map.write();
            cap_map.retain(|_, v| v != name);
        }
        removed
    }

    /// Remove all connectors.
    pub fn clear(&self) {
        self.connectors.write().clear();
        self.capability_map.write().clear();
    }

    /// Find connectors that support the given capability.
    pub fn search_by_capability(&self, capability: &CapabilityId) -> Vec<Arc<dyn Connector>> {
        self.connectors.read()
            .values()
            .filter(|c| c.descriptor().supported_capabilities.contains(capability))
            .cloned()
            .collect()
    }

    /// Find connectors by name prefix match.
    pub fn search_by_name(&self, name_prefix: &str) -> Vec<Arc<dyn Connector>> {
        self.connectors.read()
            .values()
            .filter(|c| c.descriptor().name.starts_with(name_prefix))
            .cloned()
            .collect()
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
                version: semver::Version::new(0, 10, 0),
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

    fn make_instance(cap_id: &str) -> CapabilityInstance {
        CapabilityInstance {
            contract: CapabilityContract {
                id: CapabilityId::new(cap_id),
                version: semver::Version::parse("0.1.0").unwrap(),
                description: "test".into(),
                inputs_schema: serde_json::json!({}),
                outputs_schema: serde_json::json!({}),
                permissions: vec![],
                estimated_cost_usd: 0.0,
                estimated_latency_ms: 1,
                reliability_score: 1.0,
                supports_streaming: false,
            },
            runtime_params: serde_json::json!({}),
        }
    }

    #[test]
    fn test_connector_resolver_bind_success() {
        let resolver = ConnectorResolver::new();
        let connector = Arc::new(EchoConnector {
            plugin: Arc::new(EchoPlugin::new()),
        });

        resolver.register_connector(connector).unwrap();

        let instance = make_instance("echo.text");
        let bound = resolver.bind(&instance).unwrap();
        assert_eq!(bound.connector_descriptor.name, "echo");
    }

    #[test]
    fn test_register_connector_rejects_old_version() {
        let resolver = ConnectorResolver::new();

        struct OldConnector;

        impl Connector for OldConnector {
            fn descriptor(&self) -> ConnectorDescriptor {
                ConnectorDescriptor {
                    name: "old".into(),
                    version: semver::Version::new(0, 9, 0),
                    supported_capabilities: vec![CapabilityId::new("old.test")],
                }
            }

            fn executor(&self) -> Arc<dyn CapabilityExecutor> {
                Arc::new(EchoPlugin::new())
            }
        }

        let err = resolver
            .register_connector(Arc::new(OldConnector))
            .unwrap_err();
        assert!(err.contains("0.9.0"), "error should mention version 0.9.0: {err}");
        assert!(
            err.contains("0.10.0"),
            "error should mention min version 0.10.0: {err}"
        );
    }

    #[test]
    fn test_unregister_connector_removes_and_returns_true() {
        let resolver = ConnectorResolver::new();
        let connector = Arc::new(EchoConnector {
            plugin: Arc::new(EchoPlugin::new()),
        });
        resolver.register_connector(connector).unwrap();

        assert!(resolver.unregister_connector("echo"));

        let instance = make_instance("echo.text");
        assert!(resolver.bind(&instance).is_err());
    }

    #[test]
    fn test_unregister_connector_nonexistent_returns_false() {
        let resolver = ConnectorResolver::new();
        assert!(!resolver.unregister_connector("nonexistent"));
    }

    #[test]
    fn test_unregister_connector_cleans_capability_map() {
        let resolver = ConnectorResolver::new();
        let connector = Arc::new(EchoConnector {
            plugin: Arc::new(EchoPlugin::new()),
        });
        resolver.register_connector(connector).unwrap();

        resolver.unregister_connector("echo");

        let map = resolver.capability_map.read();
        assert!(map.values().all(|v| v != "echo"));
    }

    #[test]
    fn test_search_by_capability_finds_matching_connectors() {
        let resolver = ConnectorResolver::new();
        let connector = Arc::new(EchoConnector {
            plugin: Arc::new(EchoPlugin::new()),
        });
        resolver.register_connector(connector).unwrap();

        let results = resolver.search_by_capability(&CapabilityId::new("echo.text"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].descriptor().name, "echo");

        let no_results = resolver.search_by_capability(&CapabilityId::new("nonexistent"));
        assert!(no_results.is_empty());
    }

    #[test]
    fn test_search_by_name_finds_connectors_by_prefix() {
        let resolver = ConnectorResolver::new();
        let connector = Arc::new(EchoConnector {
            plugin: Arc::new(EchoPlugin::new()),
        });
        resolver.register_connector(connector).unwrap();

        let results = resolver.search_by_name("ec");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].descriptor().name, "echo");

        let no_results = resolver.search_by_name("shell");
        assert!(no_results.is_empty());
    }

    #[test]
    fn test_clear_removes_all_connectors() {
        let resolver = ConnectorResolver::new();
        let connector = Arc::new(EchoConnector {
            plugin: Arc::new(EchoPlugin::new()),
        });
        resolver.register_connector(connector).unwrap();

        resolver.clear();

        let instance = make_instance("echo.text");
        assert!(resolver.bind(&instance).is_err());
    }
}
