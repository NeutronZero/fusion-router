use std::path::PathBuf;
use std::sync::Arc;
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
    subscribers: Vec<Box<dyn ConfigSubscriber + Send + Sync>>,
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
            subscribers,
            generation: AtomicU64::new(generation),
        }
    }

    pub fn snapshot(&self) -> ConfigSnapshot {
        self.inner.load().as_ref().clone()
    }

    fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub async fn reload(&self) -> Result<u64, ReloadError> {
        let content = std::fs::read_to_string(&self.config_path)
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

        let mut ordered: Vec<_> = self.subscribers.iter().collect();
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
