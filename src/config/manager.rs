use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use arc_swap::ArcSwap;

use super::error::ReloadError;
use super::AppConfig;

#[derive(Clone)]
pub struct ConfigSnapshot {
    pub generation: u64,
    pub config: Arc<AppConfig>,
}

pub trait ConfigSubscriber: Send + Sync {
    fn priority(&self) -> u8 { 0 }

    fn prepare(
        &self,
        old: &ConfigSnapshot,
        new: &ConfigSnapshot,
    ) -> Result<(), ReloadError>;

    fn commit(&self, generation: u64);
}

pub struct ConfigManager {
    inner: ArcSwap<ConfigSnapshot>,
    pub config_path: PathBuf,
    subscribers: RwLock<Vec<Box<dyn ConfigSubscriber + Send + Sync>>>,
    generation: AtomicU64,
}

impl ConfigManager {
    pub fn new(
        config_path: PathBuf,
        initial_config: AppConfig,
        subscribers: Vec<Box<dyn ConfigSubscriber + Send + Sync>>,
    ) -> Self {
        let generation = 1;
        let snapshot = ConfigSnapshot {
            generation,
            config: Arc::new(initial_config),
        };
        Self {
            inner: ArcSwap::new(Arc::new(snapshot)),
            config_path,
            subscribers: RwLock::new(subscribers),
            generation: AtomicU64::new(generation),
        }
    }

    pub fn snapshot(&self) -> ConfigSnapshot {
        self.inner.load().as_ref().clone()
    }

    fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn register_subscriber(&self, subscriber: Box<dyn ConfigSubscriber + Send + Sync>) {
        self.subscribers.write()
            .expect("ConfigManager subscriber lock poisoned")
            .push(subscriber);
    }

    pub async fn reload(&self) -> Result<u64, ReloadError> {
        let content = tokio::fs::read_to_string(&self.config_path)
            .await
            .map_err(|e| ReloadError::Parse(e.to_string()))?;

        let new_config: AppConfig = serde_yaml::from_str(&content)
            .map_err(|e| ReloadError::Parse(e.to_string()))?;

        new_config.validate().map_err(ReloadError::Validation)?;

        let next_gen = self.next_generation();
        let old_snapshot = self.snapshot();
        let new_snapshot = ConfigSnapshot {
            generation: next_gen,
            config: Arc::new(new_config),
        };

        let subscribers = self.subscribers.read()
            .expect("ConfigManager subscriber lock poisoned");
        let mut ordered: Vec<_> = subscribers.iter().collect();
        ordered.sort_by_key(|s| s.priority());

        for subscriber in &ordered {
            subscriber.prepare(&old_snapshot, &new_snapshot)?;
        }

        for subscriber in &ordered {
            subscriber.commit(next_gen);
        }

        self.inner.store(Arc::new(new_snapshot));
        self.generation.store(next_gen, Ordering::SeqCst);

        tracing::info!(generation = next_gen, "configuration reloaded");
        Ok(next_gen)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use crate::config::{
        error::ReloadError, AppConfig, AuthConfig, CorsConfig, LoggingConfig,
        RateLimitingConfig, ResourceConfig, ServerConfig, StrategyConfig, ToolsConfig,
    };
    use crate::config::manager::{ConfigManager, ConfigSnapshot, ConfigSubscriber};
    use crate::types::ModelCatalog;

    struct MockSubscriber {
        name: &'static str,
        priority_val: u8,
        calls: Arc<Mutex<Vec<&'static str>>>,
        fail_prepare: bool,
    }

    impl ConfigSubscriber for MockSubscriber {
        fn priority(&self) -> u8 {
            self.priority_val
        }

        fn prepare(
            &self,
            _old: &ConfigSnapshot,
            _new: &ConfigSnapshot,
        ) -> Result<(), ReloadError> {
            self.calls.lock().unwrap().push("prepare");
            if self.fail_prepare {
                Err(ReloadError::Subscriber {
                    name: self.name.into(),
                    reason: "mock failure".into(),
                })
            } else {
                Ok(())
            }
        }

        fn commit(&self, _generation: u64) {
            self.calls.lock().unwrap().push("commit");
        }
    }

    fn minimal_config() -> AppConfig {
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

    fn temp_yaml(name: &str, content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fusion_config_test_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn valid_yaml() -> String {
        r#"server:
  host: "0.0.0.0"
  port: 8080
  shutdown_timeout_secs: 30
resources:
  max_daily_cost: 100.0
  max_daily_tokens: 1000000
auth:
  enabled: false
  api_keys: []
"#
        .into()
    }

    fn invalid_yaml() -> String {
        "::invalid yaml [".into()
    }

    fn bad_validation_yaml() -> String {
        r#"server:
  host: "0.0.0.0"
  port: 0
  shutdown_timeout_secs: 30
resources:
  max_daily_cost: 100.0
  max_daily_tokens: 1000000
"#
        .into()
    }

    #[tokio::test]
    async fn test_snapshot_returns_initial_config() {
        let config = minimal_config();
        let manager = ConfigManager::new(
            std::path::PathBuf::from("/nonexistent"),
            config.clone(),
            Vec::new(),
        );

        let snapshot = manager.snapshot();

        assert_eq!(snapshot.generation, 1);
        assert_eq!(snapshot.config.server.port, 8080);
    }

    #[tokio::test]
    async fn test_reload_with_valid_config() {
        let path = temp_yaml("valid_reload.yaml", &valid_yaml());
        let manager = ConfigManager::new(path.clone(), minimal_config(), Vec::new());

        let gen = manager.reload().await.unwrap();

        assert_eq!(gen, 2);
        let snapshot = manager.snapshot();
        assert_eq!(snapshot.generation, 2);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_reload_with_invalid_yaml() {
        let path = temp_yaml("invalid_parse.yaml", &invalid_yaml());
        let manager = ConfigManager::new(path.clone(), minimal_config(), Vec::new());

        let err = manager.reload().await.unwrap_err();

        assert!(matches!(err, ReloadError::Parse(_)));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_reload_validation_failure() {
        let path = temp_yaml("bad_validation.yaml", &bad_validation_yaml());
        let manager = ConfigManager::new(path.clone(), minimal_config(), Vec::new());

        let err = manager.reload().await.unwrap_err();

        assert!(matches!(err, ReloadError::Validation(_)));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_two_phase_prepare_failure() {
        let path = temp_yaml("prepare_fail.yaml", &valid_yaml());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let subscriber = MockSubscriber {
            name: "failer",
            priority_val: 0,
            calls: calls.clone(),
            fail_prepare: true,
        };

        let manager = ConfigManager::new(
            path.clone(),
            minimal_config(),
            vec![Box::new(subscriber)],
        );

        let old_snapshot = manager.snapshot();
        let err = manager.reload().await.unwrap_err();

        assert!(matches!(err, ReloadError::Subscriber { .. }));

        // Snapshot must remain unchanged
        let new_snapshot = manager.snapshot();
        assert_eq!(new_snapshot.generation, old_snapshot.generation);

        // commit() must NOT be called
        let recorded = calls.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], "prepare");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_two_phase_commit_success() {
        let path = temp_yaml("commit_success.yaml", &valid_yaml());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let subscriber = MockSubscriber {
            name: "success",
            priority_val: 0,
            calls: calls.clone(),
            fail_prepare: false,
        };

        let manager = ConfigManager::new(
            path.clone(),
            minimal_config(),
            vec![Box::new(subscriber)],
        );

        let gen = manager.reload().await.unwrap();

        assert_eq!(gen, 2);

        let recorded = calls.lock().unwrap();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0], "prepare");
        assert_eq!(recorded[1], "commit");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_subscriber_priority_ordering() {
        let path = temp_yaml("priority.yaml", &valid_yaml());
        let high_calls = Arc::new(Mutex::new(Vec::new()));
        let low_calls = Arc::new(Mutex::new(Vec::new()));

        let high = MockSubscriber {
            name: "high",
            priority_val: 0,
            calls: high_calls.clone(),
            fail_prepare: false,
        };
        let low = MockSubscriber {
            name: "low",
            priority_val: 1,
            calls: low_calls.clone(),
            fail_prepare: false,
        };

        let manager = ConfigManager::new(
            path.clone(),
            minimal_config(),
            vec![Box::new(high), Box::new(low)],
        );

        manager.reload().await.unwrap();

        let high_log = high_calls.lock().unwrap();
        let low_log = low_calls.lock().unwrap();

        assert_eq!(high_log.len(), 2);
        assert_eq!(low_log.len(), 2);
        assert_eq!(high_log[0], "prepare");
        assert_eq!(high_log[1], "commit");
        assert_eq!(low_log[0], "prepare");
        assert_eq!(low_log[1], "commit");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_register_subscriber() {
        let path = temp_yaml("register.yaml", &valid_yaml());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let subscriber = MockSubscriber {
            name: "late",
            priority_val: 0,
            calls: calls.clone(),
            fail_prepare: false,
        };

        let manager = ConfigManager::new(path.clone(), minimal_config(), Vec::new());
        manager.register_subscriber(Box::new(subscriber));

        let gen = manager.reload().await.unwrap();

        assert_eq!(gen, 2);

        let recorded = calls.lock().unwrap();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0], "prepare");
        assert_eq!(recorded[1], "commit");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn test_concurrent_snapshot_isolation() {
        let path = temp_yaml("concurrent.yaml", &valid_yaml());
        let manager = std::sync::Arc::new(ConfigManager::new(
            path.clone(),
            minimal_config(),
            Vec::new(),
        ));

        let before = manager.snapshot();
        assert_eq!(before.generation, 1);

        let mgr_clone = manager.clone();
        let reload_task = tokio::spawn(async move {
            mgr_clone.reload().await.unwrap()
        });

        let mut snapshot_tasks = Vec::new();
        for _ in 0..10 {
            let mgr = manager.clone();
            snapshot_tasks.push(tokio::spawn(async move {
                mgr.snapshot()
            }));
        }

        let reload_gen = reload_task.await.unwrap();
        assert_eq!(reload_gen, 2);

        for task in snapshot_tasks {
            let snap = task.await.unwrap();
            assert!(
                snap.generation == 1 || snap.generation == 2,
                "snapshot generation must be 1 or 2, got {}",
                snap.generation
            );
        }

        let after = manager.snapshot();
        assert_eq!(after.generation, 2);
        let _ = std::fs::remove_file(&path);
    }
}
