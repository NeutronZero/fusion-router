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
    index: RwLock<Index>,
    similarity_threshold: f32,
    max_entries: usize,
    next_label: AtomicU64,
}

impl SemanticCache {
    pub fn new(
        embedder: Arc<dyn Embedder + Send + Sync>,
        similarity_threshold: f32,
        max_entries: usize,
        dimensions: usize,
    ) -> Self {
        let options = IndexOptions {
            dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
            multi: false,
        };
        let index = Index::new(&options).expect("Failed to create HNSW index");
        index.reserve(max_entries).expect("Failed to reserve index capacity");
        Self {
            embedder,
            entries: RwLock::new(HashMap::new()),
            index: RwLock::new(index),
            similarity_threshold,
            max_entries,
            next_label: AtomicU64::new(0),
        }
    }

    pub async fn get(&self, query: &str) -> Option<Value> {
        let query_embedding = self.embedder.embed(query).await.ok()?;
        let entries = self.entries.read();
        if entries.is_empty() {
            return None;
        }
        let index = self.index.read();
        let results = index.search(&query_embedding, 1).ok()?;
        let label = *results.keys.first()?;
        if let Some(entry) = entries.get(&label) {
            let score = cosine_similarity(&query_embedding, &entry.embedding);
            if score >= self.similarity_threshold {
                return Some(entry.response.clone());
            }
        }
        None
    }

    pub async fn put(&self, key: &str, response: Value) {
        if let Ok(embedding) = self.embedder.embed(key).await {
            let mut entries = self.entries.write();
            let index = self.index.write();
            if entries.len() >= self.max_entries {
                if let Some(&oldest) = entries.keys().min() {
                    let _ = index.remove(oldest);
                    entries.remove(&oldest);
                }
            }
            let label = self.next_label.fetch_add(1, Ordering::Relaxed);
            let _ = index.add(label, &embedding);
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

    pub fn clear(&self) {
        self.entries.write().clear();
        let _ = self.index.write().reset();
        self.next_label.store(0, Ordering::Relaxed);
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
}
