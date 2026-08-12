use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use serde_json::Value;
use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

use super::embeddings::{cosine_similarity, Embedder};

pub struct CacheEntry {
    pub embedding: Vec<f32>,
    pub response: Value,
    #[allow(dead_code)]
    pub key: String,
}

pub struct SemanticCache {
    embedder: Arc<dyn Embedder + Send + Sync>,
    entries: RwLock<HashMap<u64, CacheEntry>>,
    index: Arc<std::sync::Mutex<Index>>,
    similarity_threshold: f32,
    max_entries: usize,
    next_label: AtomicU64,
    dimensions: usize,
}

impl SemanticCache {
    pub fn try_new(
        embedder: Arc<dyn Embedder + Send + Sync>,
        similarity_threshold: f32,
        max_entries: usize,
        dimensions: usize,
    ) -> Result<Self, String> {
        let options = IndexOptions {
            dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
            multi: false,
        };
        let index = Index::new(&options).map_err(|e| format!("Failed to create HNSW index: {}", e))?;
        index.reserve(max_entries).map_err(|e| format!("Failed to reserve index capacity: {}", e))?;
        Ok(Self {
            embedder,
            entries: RwLock::new(HashMap::new()),
            index: Arc::new(std::sync::Mutex::new(index)),
            similarity_threshold,
            max_entries,
            next_label: AtomicU64::new(0),
            dimensions,
        })
    }

    pub fn new(
        embedder: Arc<dyn Embedder + Send + Sync>,
        similarity_threshold: f32,
        max_entries: usize,
        dimensions: usize,
    ) -> Self {
        Self::try_new(embedder.clone(), similarity_threshold, max_entries, dimensions).unwrap_or_else(|e| {
            tracing::error!(error = %e, "Failed to initialize HNSW index for SemanticCache");
            let options = IndexOptions {
                dimensions,
                metric: MetricKind::Cos,
                quantization: ScalarKind::F32,
                connectivity: 16,
                expansion_add: 128,
                expansion_search: 64,
                multi: false,
            };
            let index = Index::new(&options).unwrap_or_else(|err| {
                tracing::error!(error = %err, "Failed fallback Index creation");
                Index::new(&IndexOptions {
                    dimensions,
                    metric: MetricKind::Cos,
                    quantization: ScalarKind::F32,
                    connectivity: 2,
                    expansion_add: 2,
                    expansion_search: 2,
                    multi: false,
                })
                .unwrap_or_else(|e| {
                    tracing::error!(error = %e, "Failed to create minimal HNSW index");
                    Index::new(&IndexOptions::default()).unwrap_or_else(|e2| {
                        tracing::error!(error = %e2, "Failed to create default HNSW index");
                        Index::new(&IndexOptions {
                            dimensions: 1,
                            metric: MetricKind::Cos,
                            quantization: ScalarKind::F32,
                            connectivity: 2,
                            expansion_add: 2,
                            expansion_search: 2,
                            multi: false,
                        }).unwrap_or_else(|e3| {
                            tracing::error!(error = %e3, "Failed to allocate minimal HNSW index");
                            std::process::exit(1);
                        })
                    })
                })
            });
            Self {
                embedder,
                entries: RwLock::new(HashMap::new()),
                index: Arc::new(std::sync::Mutex::new(index)),
                similarity_threshold,
                max_entries,
                next_label: AtomicU64::new(0),
                dimensions,
            }
        })
    }

    pub async fn get(&self, query: &str) -> Option<Value> {
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
            let idx = index.lock().unwrap_or_else(|e| e.into_inner());
            idx.search(&emb, 1)
        })
        .await
        .ok()?.ok()?;

        let label = *results.keys.first()?;
        let entries = self.entries.read();
        let entry = entries.get(&label)?;
        let score = cosine_similarity(&query_embedding, &entry.embedding);
        if score >= self.similarity_threshold {
            Some(entry.response.clone())
        } else {
            None
        }
    }

    pub async fn put(&self, key: &str, response: Value) {
        if let Ok(embedding) = self.embedder.embed(key).await {
            let label = self.next_label.fetch_add(1, Ordering::Relaxed);

            let index = self.index.clone();
            let emb = embedding.clone();
            if tokio::task::spawn_blocking(move || {
                let idx = index.lock().unwrap_or_else(|e| e.into_inner());
                idx.add(label, &emb)
            })
            .await
            .is_err()
            {
                return;
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
                let _ = tokio::task::spawn_blocking(move || {
                    let idx = index.lock().unwrap_or_else(|e| e.into_inner());
                    let _ = idx.remove(oldest);
                })
                .await;
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
                    key: key.to_string(),
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
        let options = IndexOptions {
            dimensions: self.dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
            multi: false,
        };
        let new_index = Index::new(&options).map_err(|e| format!("Failed to create new HNSW index: {}", e))?;
        new_index.reserve(self.max_entries).map_err(|e| format!("Failed to reserve index capacity: {}", e))?;
        self.entries.write().clear();
        *self.index.lock().unwrap_or_else(|e| e.into_inner()) = new_index;
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

    #[tokio::test]
    async fn test_cache_miss_on_empty() {
        let cache = SemanticCache::new(Arc::new(MockEmbedder), 0.9, 100, 384);
        let result = cache.get("test query").await;
        assert!(result.is_none(), "Empty cache should return None");
    }

    #[tokio::test]
    async fn test_cache_hit_after_put() {
        let cache = SemanticCache::new(Arc::new(MockEmbedder), 0.0, 100, 384);
        cache.put("test query", serde_json::json!("cached response")).await;
        let result = cache.get("test query").await;
        assert!(result.is_some(), "Should find cached response");
        assert_eq!(result.unwrap(), serde_json::json!("cached response"));
    }

    #[tokio::test]
    async fn test_cache_eviction() {
        let cache = SemanticCache::new(Arc::new(MockEmbedder), 0.0, 2, 384);
        cache.put("key1", serde_json::json!("r1")).await;
        cache.put("key2", serde_json::json!("r2")).await;
        cache.put("key3", serde_json::json!("r3")).await;
        assert_eq!(cache.len(), 2, "Should evict oldest entry leaving newest 2");
    }

    #[test]
    fn test_set_capacity() {
        let mut cache = SemanticCache::new(Arc::new(MockEmbedder), 0.9, 100, 384);
        assert_eq!(cache.max_entries, 100);
        cache.set_capacity(50);
        assert_eq!(cache.max_entries, 50);
    }
}
