use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde_json::Value;
use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

use super::embeddings::{cosine_similarity, Embedder};

/// Default time-to-live for cached responses. Entries older than this are
/// treated as misses and lazily evicted on lookup.
pub const DEFAULT_TTL_SECS: u64 = 3600;

pub struct CacheEntry {
    pub embedding: Vec<f32>,
    pub response: Value,
    /// Provider usage metrics captured with the response so token telemetry
    /// survives a cache hit.
    pub usage: Option<Value>,
    #[allow(dead_code)]
    pub key: String,
    /// Insertion timestamp used for TTL enforcement.
    pub inserted_at: Instant,
}

/// A cache hit: the cached response plus the usage recorded alongside it.
#[derive(Debug, Clone)]
pub struct CacheHit {
    pub response: Value,
    pub usage: Option<Value>,
}

fn open_index(options: &IndexOptions) -> Result<Index, String> {
    let index = Index::new(options).map_err(|e| format!("Failed to create HNSW index: {}", e))?;
    Ok(index)
}

fn default_index_options(dimensions: usize) -> IndexOptions {
    IndexOptions {
        dimensions,
        metric: MetricKind::Cos,
        quantization: ScalarKind::F32,
        connectivity: 16,
        expansion_add: 128,
        expansion_search: 64,
        multi: false,
    }
}

fn minimal_index_options(dimensions: usize) -> IndexOptions {
    IndexOptions {
        dimensions,
        metric: MetricKind::Cos,
        quantization: ScalarKind::F32,
        connectivity: 2,
        expansion_add: 2,
        expansion_search: 2,
        multi: false,
    }
}

pub struct SemanticCache {
    embedder: Arc<dyn Embedder + Send + Sync>,
    entries: RwLock<HashMap<u64, CacheEntry>>,
    index: Arc<parking_lot::Mutex<Index>>,
    similarity_threshold: f32,
    max_entries: usize,
    next_label: AtomicU64,
    dimensions: usize,
    ttl: Duration,
}

impl SemanticCache {
    /// Preferred constructor: returns Err instead of aborting the process
    /// when the HNSW index cannot be initialized.
    pub fn try_new(
        embedder: Arc<dyn Embedder + Send + Sync>,
        similarity_threshold: f32,
        max_entries: usize,
        dimensions: usize,
    ) -> Result<Self, String> {
        let options = default_index_options(dimensions);
        let index = open_index(&options)?;
        index
            .reserve(max_entries)
            .map_err(|e| format!("Failed to reserve index capacity: {}", e))?;
        Ok(Self {
            embedder,
            entries: RwLock::new(HashMap::new()),
            index: Arc::new(parking_lot::Mutex::new(index)),
            similarity_threshold,
            max_entries,
            next_label: AtomicU64::new(0),
            dimensions,
            ttl: Duration::from_secs(DEFAULT_TTL_SECS),
        })
    }

    /// Convenience constructor with a degraded-mode fallback (smaller HNSW
    /// graph parameters). Returns `Err` on total initialization failure —
    /// this function must never terminate the process from inside library
    /// code; callers decide how to degrade.
    pub fn new(
        embedder: Arc<dyn Embedder + Send + Sync>,
        similarity_threshold: f32,
        max_entries: usize,
        dimensions: usize,
    ) -> Result<Self, String> {
        Self::try_new(
            embedder.clone(),
            similarity_threshold,
            max_entries,
            dimensions,
        )
        .or_else(|primary_err| {
            tracing::warn!(
                error = %primary_err,
                "SemanticCache primary init failed; retrying with minimal HNSW parameters"
            );
            let options = minimal_index_options(dimensions);
            let index = open_index(&options)?;
            index.reserve(max_entries).map_err(|e| {
                format!(
                    "SemanticCache init failed (primary: {primary_err}); \
                         minimal fallback reserve failed: {e}"
                )
            })?;
            tracing::warn!("SemanticCache initialized in degraded (minimal HNSW) mode");
            Ok(Self {
                embedder,
                entries: RwLock::new(HashMap::new()),
                index: Arc::new(parking_lot::Mutex::new(index)),
                similarity_threshold,
                max_entries,
                next_label: AtomicU64::new(0),
                dimensions,
                ttl: Duration::from_secs(DEFAULT_TTL_SECS),
            })
        })
    }

    /// Overrides the entry time-to-live (mainly for tests).
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Full-fidelity lookup: returns the cached response together with the
    /// usage metrics stored alongside it. Expired entries are treated as
    /// misses and lazily evicted.
    pub async fn lookup(&self, query: &str) -> Option<CacheHit> {
        let query_embedding = self.embedder.embed(query).await.ok()?;
        {
            let entries = self.entries.read();
            if entries.is_empty() {
                return None;
            }
        }

        let index = self.index.clone();
        let emb = query_embedding.clone();
        let results = tokio::task::spawn_blocking(move || {
            let idx = index.lock();
            idx.search(&emb, 1)
        })
        .await
        .ok()?
        .ok()?;

        let label = *results.keys.first()?;
        let (response, entry_embedding, usage, expired) = {
            let entries = self.entries.read();
            match entries.get(&label) {
                Some(entry) => (
                    Some(entry.response.clone()),
                    Some(entry.embedding.clone()),
                    Some(entry.usage.clone()),
                    entry.inserted_at.elapsed() >= self.ttl,
                ),
                None => (None, None, None, false),
            }
        };

        if expired {
            // Lazy eviction: drop the stale entry so it cannot shadow fresh ones.
            let removed = {
                let mut entries = self.entries.write();
                entries.remove(&label).is_some()
            };
            if removed {
                let index = self.index.clone();
                let remove_result = tokio::task::spawn_blocking(move || {
                    let idx = index.lock();
                    idx.remove(label)
                })
                .await;
                match remove_result {
                    Ok(Ok(_removed)) => {}
                    Ok(Err(e)) => tracing::debug!(
                        error = %e,
                        label = %label,
                        "HNSW remove of expired cache entry failed"
                    ),
                    Err(e) => tracing::debug!(
                        error = %e,
                        label = %label,
                        "HNSW expired-entry remove task failed"
                    ),
                }
            }
            return None;
        }

        let (response, usage, entry_embedding) = (response?, usage.flatten(), entry_embedding?);
        let score = cosine_similarity(&query_embedding, &entry_embedding);
        if score >= self.similarity_threshold {
            Some(CacheHit { response, usage })
        } else {
            None
        }
    }

    /// Back-compat lookup returning only the response payload.
    pub async fn get(&self, query: &str) -> Option<Value> {
        self.lookup(query).await.map(|hit| hit.response)
    }

    pub async fn put(&self, key: &str, response: Value) {
        self.put_with_usage(key, response, None).await;
    }

    pub async fn put_with_usage(&self, key: &str, response: Value, usage: Option<Value>) {
        if let Ok(embedding) = self.embedder.embed(key).await {
            let label = self.next_label.fetch_add(1, Ordering::Relaxed);

            let index = self.index.clone();
            let emb = embedding.clone();
            let add_result = tokio::task::spawn_blocking(move || {
                let idx = index.lock();
                idx.add(label, &emb)
            })
            .await;
            match add_result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "HNSW add failed; cache entry skipped");
                    return;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "HNSW add task failed; cache entry skipped");
                    return;
                }
            }

            let oldest_to_remove = {
                let entries = self.entries.read();
                if entries.len() >= self.max_entries {
                    entries.keys().min().copied()
                } else {
                    None
                }
            };

            if let Some(oldest) = oldest_to_remove {
                let index = self.index.clone();
                let remove_result = tokio::task::spawn_blocking(move || {
                    let idx = index.lock();
                    idx.remove(oldest)
                })
                .await;
                match remove_result {
                    Ok(Ok(_removed)) => {}
                    Ok(Err(e)) => tracing::debug!(
                        error = %e,
                        label = %oldest,
                        "HNSW remove of evicted cache entry failed"
                    ),
                    Err(e) => tracing::debug!(
                        error = %e,
                        label = %oldest,
                        "HNSW evicted-entry remove task failed"
                    ),
                }
            }

            let mut entries = self.entries.write();
            if let Some(oldest) = oldest_to_remove {
                entries.remove(&oldest);
            }
            entries.insert(
                label,
                CacheEntry {
                    embedding,
                    response,
                    usage,
                    key: key.to_string(),
                    inserted_at: Instant::now(),
                },
            );
        }
    }

    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.read().is_empty()
    }

    pub fn try_clear(&self) -> Result<(), String> {
        let options = default_index_options(self.dimensions);
        let new_index = open_index(&options)
            .map_err(|e| format!("Failed to create replacement HNSW index during clear: {e}"))?;
        new_index
            .reserve(self.max_entries)
            .map_err(|e| format!("Failed to reserve index capacity: {}", e))?;
        self.entries.write().clear();
        *self.index.lock() = new_index;
        self.next_label.store(0, Ordering::Relaxed);
        Ok(())
    }

    pub fn clear(&self) {
        let _ = self.try_clear();
    }

    pub fn set_capacity(&mut self, new_capacity: usize) {
        self.max_entries = new_capacity;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::embeddings::MockEmbedder;

    fn cache(threshold: f32, max: usize) -> SemanticCache {
        SemanticCache::new(Arc::new(MockEmbedder), threshold, max, 384)
            .expect("semantic cache init")
    }

    #[tokio::test]
    async fn test_cache_miss_on_empty() {
        let cache = cache(0.9, 100);
        let result = cache.get("test query").await;
        assert!(result.is_none(), "Empty cache should return None");
    }

    #[tokio::test]
    async fn test_cache_hit_after_put() {
        let cache = cache(0.0, 100);
        cache
            .put("test query", serde_json::json!("cached response"))
            .await;
        let result = cache.get("test query").await;
        assert!(result.is_some(), "Should find cached response");
        assert_eq!(result.unwrap(), serde_json::json!("cached response"));
    }

    #[tokio::test]
    async fn test_cache_eviction() {
        let cache = cache(0.0, 2);
        cache.put("key1", serde_json::json!("r1")).await;
        cache.put("key2", serde_json::json!("r2")).await;
        cache.put("key3", serde_json::json!("r3")).await;
        assert_eq!(cache.len(), 2, "Should evict oldest entry leaving newest 2");
    }

    #[tokio::test]
    async fn test_ttl_expiry_treats_entry_as_miss_and_evicts() {
        let cache = cache(0.0, 100).with_ttl(Duration::from_millis(50));
        cache.put("ttl query", serde_json::json!("fresh")).await;
        let hit = cache.lookup("ttl query").await;
        assert!(hit.is_some(), "entry inside TTL window must hit");
        assert_eq!(hit.unwrap().response, serde_json::json!("fresh"));

        tokio::time::sleep(Duration::from_millis(80)).await;
        let after_expiry = cache.lookup("ttl query").await;
        assert!(after_expiry.is_none(), "expired entry must miss");
        assert_eq!(
            cache.len(),
            0,
            "expired entry must be lazily evicted on lookup"
        );
    }

    #[tokio::test]
    async fn test_usage_round_trips_on_hit() {
        let cache = cache(0.0, 100);
        let usage = serde_json::json!({
            "prompt_tokens": 11,
            "completion_tokens": 7,
            "total_tokens": 18
        });
        cache
            .put_with_usage(
                "usage query",
                serde_json::json!({"content": "hello"}),
                Some(usage.clone()),
            )
            .await;

        let hit = cache.lookup("usage query").await.expect("must hit");
        assert_eq!(hit.response, serde_json::json!({"content": "hello"}));
        assert_eq!(hit.usage, Some(usage));

        // Legacy get() still returns just the response payload.
        let legacy = cache.get("usage query").await.unwrap();
        assert_eq!(legacy["content"], "hello");
    }

    #[tokio::test]
    async fn test_put_without_usage_yields_none_usage_on_hit() {
        let cache = cache(0.0, 100);
        cache
            .put("plain", serde_json::json!({"content": "x"}))
            .await;
        let hit = cache.lookup("plain").await.unwrap();
        assert!(hit.usage.is_none());
    }

    #[test]
    fn test_set_capacity() {
        let mut cache = cache(0.9, 100);
        assert_eq!(cache.max_entries, 100);
        cache.set_capacity(50);
        assert_eq!(cache.max_entries, 50);
    }

    #[test]
    fn test_default_ttl_constant_is_one_hour() {
        assert_eq!(DEFAULT_TTL_SECS, 3600);
    }
}
