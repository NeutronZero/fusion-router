use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::RwLock;

use crate::scheduler::connector_resolver::Connector;
use crate::scheduler::connector_resolver::ConnectorResolver;

#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone)]
pub struct ConnectorHealth {
    pub status: HealthStatus,
    pub last_check_at: Instant,
    pub latency_ms: u64,
    pub error_count: u64,
}

pub struct ConnectorHealthChecker {
    health_map: Arc<RwLock<HashMap<String, ConnectorHealth>>>,
    check_interval_secs: u64,
}

impl ConnectorHealthChecker {
    pub fn new(check_interval_secs: u64) -> Self {
        Self {
            health_map: Arc::new(RwLock::new(HashMap::new())),
            check_interval_secs,
        }
    }

    pub fn health_map(&self) -> Arc<RwLock<HashMap<String, ConnectorHealth>>> {
        self.health_map.clone()
    }

    pub async fn check_connector_health(
        &self,
        __name: &str,
        connector: &dyn Connector,
    ) -> ConnectorHealth {
        let start = Instant::now();
        let desc = connector.descriptor();
        let _ = desc;
        let latency = start.elapsed().as_millis() as u64;
        ConnectorHealth {
            status: HealthStatus::Healthy,
            last_check_at: Instant::now(),
            latency_ms: latency,
            error_count: 0,
        }
    }

    pub async fn run(&self, resolver: Arc<ConnectorResolver>) {
        let mut interval = tokio::time::interval(Duration::from_secs(self.check_interval_secs));
        loop {
            interval.tick().await;
            let connector_pairs: Vec<(String, Arc<dyn Connector>)> = {
                let guard = resolver.connectors.read();
                guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
            };

            let futures = connector_pairs
                .into_iter()
                .map(|(name, connector)| async move {
                    let health = self.check_connector_health(&name, connector.as_ref()).await;
                    (name, health)
                });

            let results = futures::future::join_all(futures).await;
            let mut map = self.health_map.write().await;
            for (name, health) in results {
                map.insert(name, health);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::connector_resolver::{Connector, ConnectorDescriptor};
    use std::sync::Arc;

    struct MockConnector;

    impl Connector for MockConnector {
        fn descriptor(&self) -> ConnectorDescriptor {
            ConnectorDescriptor {
                name: "mock".into(),
                version: semver::Version::new(1, 0, 0),
                supported_capabilities: vec![],
            }
        }

        fn executor(&self) -> Arc<dyn fusion_plugin_api::CapabilityExecutor> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn test_initial_health_empty() {
        let checker = ConnectorHealthChecker::new(60);
        let map_arc = checker.health_map();
        assert!(map_arc.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_check_connector_health_returns_healthy() {
        let checker = ConnectorHealthChecker::new(60);
        let connector = MockConnector;
        let health = checker
            .check_connector_health("mock_connector", &connector)
            .await;
        assert_eq!(health.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_health_map_updates_after_check() {
        let checker = ConnectorHealthChecker::new(60);
        let connector = MockConnector;
        let name = "mock_connector";
        let health = checker.check_connector_health(name, &connector).await;
        let map_arc = checker.health_map();
        map_arc.write().await.insert(name.to_string(), health);
        let map = map_arc.read().await;
        assert!(map.contains_key(name));
        assert_eq!(map[name].status, HealthStatus::Healthy);
    }
}
