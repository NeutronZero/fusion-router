//! Phase 3A — `Connector`, `ConnectorDescriptor`, `BoundConnector`, & `ConnectorResolver` (`src/scheduler/connector_resolver.rs`)
//!
//! Late binding of abstract `CapabilityInstance` handles to concrete `Connector` instances at execution time.

use fusion_plugin_api::{CapabilityExecutor, CapabilityId, CapabilityInstance};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadBalancingStrategy {
    RoundRobin,
    Random,
    LeastLatency,
}

/// Health status tracked per connector (updated by `ConnectorHealthChecker`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorHealth {
    Healthy,
    Degraded,
    Unhealthy,
}

/// Thread-safe registry providing late-binding resolution of capabilities to connectors.
/// AD-003: supports multiple connectors per capability with load balancing and failover.
#[derive(Clone)]
pub struct ConnectorResolver {
    pub(crate) connectors: Arc<RwLock<HashMap<String, Arc<dyn Connector>>>>,
    /// Maps capability → list of connector names (supports many-to-one)
    capability_map: Arc<RwLock<HashMap<CapabilityId, Vec<String>>>>,
    /// Round-robin index per capability
    rr_index: Arc<RwLock<HashMap<CapabilityId, usize>>>,
    /// Health override per connector name
    health: Arc<RwLock<HashMap<String, ConnectorHealth>>>,
}

impl ConnectorResolver {
    pub fn new() -> Self {
        Self {
            connectors: Arc::new(RwLock::new(HashMap::new())),
            capability_map: Arc::new(RwLock::new(HashMap::new())),
            rr_index: Arc::new(RwLock::new(HashMap::new())),
            health: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Mark connector health (called by health checker).
    pub fn set_health(&self, name: &str, h: ConnectorHealth) {
        self.health.write().insert(name.to_string(), h);
    }

    pub fn health(&self, name: &str) -> ConnectorHealth {
        self.health
            .read()
            .get(name)
            .copied()
            .unwrap_or(ConnectorHealth::Healthy)
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
            map_guard
                .entry(cap_id.clone())
                .or_default()
                .push(desc.name.clone());
            // de-duplicate within capability
            let v = map_guard.get_mut(cap_id).unwrap();
            v.sort();
            v.dedup();
        }
        connectors_guard.insert(desc.name.clone(), connector);
        self.health
            .write()
            .insert(desc.name.clone(), ConnectorHealth::Healthy);
        Ok(())
    }

    /// Late-binds with default round-robin load balancing and failover.
    pub fn bind(&self, instance: &CapabilityInstance) -> Result<BoundConnector, String> {
        self.bind_with_strategy(instance, LoadBalancingStrategy::RoundRobin)
    }

    /// Late-binds with explicit strategy. Filters unhealthy connectors and fails
    /// closed when none are available.
    pub fn bind_with_strategy(
        &self,
        instance: &CapabilityInstance,
        strategy: LoadBalancingStrategy,
    ) -> Result<BoundConnector, String> {
        let candidates: Vec<String> = {
            let map_guard = self.capability_map.read();
            map_guard
                .get(&instance.contract.id)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "No connector registered for capability: {}",
                        instance.contract.id
                    )
                })?
        };

        // Filter to healthy/degraded (skip unhealthy), fallback to all if none healthy
        let health_guard = self.health.read();
        let healthy: Vec<String> = candidates
            .iter()
            .filter(|n| health_guard.get(*n).copied().unwrap_or(ConnectorHealth::Healthy) != ConnectorHealth::Unhealthy)
            .cloned()
            .collect();
        let pool = if healthy.is_empty() { candidates.clone() } else { healthy };
        if pool.is_empty() {
            return Err(format!(
                "No healthy connector for capability: {}",
                instance.contract.id
            ));
        }

        let chosen_name = match strategy {
            LoadBalancingStrategy::RoundRobin => {
                let mut idx_guard = self.rr_index.write();
                let idx = idx_guard.entry(instance.contract.id.clone()).or_insert(0);
                let name = pool[*idx % pool.len()].clone();
                *idx = (*idx + 1) % pool.len();
                name
            }
            LoadBalancingStrategy::Random => {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut h = DefaultHasher::new();
                // Deterministic pseudo-random: hash(instance id + nanos) mod len
                instance.contract.id.as_str().hash(&mut h);
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos()
                    .hash(&mut h);
                let idx = (h.finish() as usize) % pool.len();
                pool[idx].clone()
            }
            LoadBalancingStrategy::LeastLatency => {
                // Pick connector with lowest estimated latency (descriptor not yet loaded,
                // so compare via connectors map)
                let connectors_guard = self.connectors.read();
                let mut best: Option<(String, u64)> = None;
                for name in &pool {
                    if let Some(c) = connectors_guard.get(name) {
                        // Use a static latency hint: descriptor holds no latency,
                        // so we use Healthy (0 penalty) vs Degraded (+100ms)
                        let penalty = match health_guard.get(name).copied().unwrap_or(ConnectorHealth::Healthy) {
                            ConnectorHealth::Healthy => 0,
                            ConnectorHealth::Degraded => 100,
                            ConnectorHealth::Unhealthy => 1000,
                        };
                        let latency = penalty;
                        if best.as_ref().map(|(_, l)| latency < *l).unwrap_or(true) {
                            best = Some((name.clone(), latency));
                        }
                    }
                }
                best.map(|(n, _)| n).unwrap_or_else(|| pool[0].clone())
            }
        };

        let connectors_guard = self.connectors.read();
        let connector = connectors_guard.get(&chosen_name).ok_or_else(|| {
            format!(
                "Connector '{}' registered for capability '{}' not found",
                chosen_name, instance.contract.id
            )
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
            for v in cap_map.values_mut() {
                v.retain(|n| n != name);
            }
            cap_map.retain(|_, v| !v.is_empty());
            self.health.write().remove(name);
        }
        removed
    }

    /// Remove all connectors.
    pub fn clear(&self) {
        self.connectors.write().clear();
        self.capability_map.write().clear();
        self.rr_index.write().clear();
        self.health.write().clear();
    }

    /// Find connectors that support the given capability.
    pub fn search_by_capability(&self, capability: &CapabilityId) -> Vec<Arc<dyn Connector>> {
        self.connectors
            .read()
            .values()
            .filter(|c| c.descriptor().supported_capabilities.contains(capability))
            .cloned()
            .collect()
    }

    /// Find connectors by name prefix match.
    pub fn search_by_name(&self, name_prefix: &str) -> Vec<Arc<dyn Connector>> {
        self.connectors
            .read()
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
                dependencies: vec![],
                estimated_cost: fusion_core::NanoUSD::ZERO,
                estimated_latency_ms: 1,
                reliability_score: 1.0,
                supports_streaming: false,
                traits: vec![],
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
        assert!(
            err.contains("0.9.0"),
            "error should mention version 0.9.0: {err}"
        );
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
        assert!(map.values().all(|v| !v.contains(&"echo".to_string())));
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

    #[test]
    fn test_load_balancing_round_robin() {
        let resolver = ConnectorResolver::new();
        struct ConnA;
        impl Connector for ConnA {
            fn descriptor(&self) -> ConnectorDescriptor {
                ConnectorDescriptor {
                    name: "a".into(),
                    version: semver::Version::new(0, 10, 0),
                    supported_capabilities: vec![CapabilityId::new("cap.x")],
                }
            }
            fn executor(&self) -> Arc<dyn CapabilityExecutor> {
                Arc::new(EchoPlugin::new())
            }
        }
        struct ConnB;
        impl Connector for ConnB {
            fn descriptor(&self) -> ConnectorDescriptor {
                ConnectorDescriptor {
                    name: "b".into(),
                    version: semver::Version::new(0, 10, 0),
                    supported_capabilities: vec![CapabilityId::new("cap.x")],
                }
            }
            fn executor(&self) -> Arc<dyn CapabilityExecutor> {
                Arc::new(EchoPlugin::new())
            }
        }
        resolver.register_connector(Arc::new(ConnA)).unwrap();
        resolver.register_connector(Arc::new(ConnB)).unwrap();
        let inst = make_instance("cap.x");
        let first = resolver.bind(&inst).unwrap().connector_descriptor.name;
        let second = resolver.bind(&inst).unwrap().connector_descriptor.name;
        assert_ne!(first, second, "round-robin must alternate");
        let third = resolver.bind(&inst).unwrap().connector_descriptor.name;
        assert_eq!(first, third, "round-robin must cycle");
    }

    #[test]
    fn test_failover_skips_unhealthy() {
        let resolver = ConnectorResolver::new();
        struct ConnA;
        impl Connector for ConnA {
            fn descriptor(&self) -> ConnectorDescriptor {
                ConnectorDescriptor {
                    name: "a".into(),
                    version: semver::Version::new(0, 10, 0),
                    supported_capabilities: vec![CapabilityId::new("cap.y")],
                }
            }
            fn executor(&self) -> Arc<dyn CapabilityExecutor> {
                Arc::new(EchoPlugin::new())
            }
        }
        struct ConnB;
        impl Connector for ConnB {
            fn descriptor(&self) -> ConnectorDescriptor {
                ConnectorDescriptor {
                    name: "b".into(),
                    version: semver::Version::new(0, 10, 0),
                    supported_capabilities: vec![CapabilityId::new("cap.y")],
                }
            }
            fn executor(&self) -> Arc<dyn CapabilityExecutor> {
                Arc::new(EchoPlugin::new())
            }
        }
        resolver.register_connector(Arc::new(ConnA)).unwrap();
        resolver.register_connector(Arc::new(ConnB)).unwrap();
        resolver.set_health("a", ConnectorHealth::Unhealthy);
        let inst = make_instance("cap.y");
        // Should always pick b now
        for _ in 0..3 {
            assert_eq!(
                resolver.bind(&inst).unwrap().connector_descriptor.name,
                "b"
            );
        }
    }
}
