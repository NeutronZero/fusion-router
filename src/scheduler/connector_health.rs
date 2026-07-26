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
            let connectors = resolver.connectors.read().clone();
            for (name, connector) in &connectors {
                let health = self.check_connector_health(name, connector.as_ref()).await;
                self.health_map.write().await.insert(name.clone(), health);
            }
        }
    }
}