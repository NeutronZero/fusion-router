use std::sync::Arc;

use parking_lot::RwLock;
use serde_json::Value;

use super::embeddings::{cosine_similarity, Embedder};

pub struct CacheEntry {
    pub embedding: Vec<f32>,
    pub response: Value,
    pub key: String,
}

pub struct SemanticCache {
    embedder: Arc<dyn Embedder + Send + Sync>,
    entries: RwLock<Vec<CacheEntry>>,
    similarity_threshold: f32,
    max_entries: usize,
}

impl SemanticCache {
    pub fn new(
        embedder: Arc<dyn Embedder + Send + Sync>,
        similarity_threshold: f32,
        max_entries: usize,
    ) -> Self {
        Self {
            embedder,
            entries: RwLock::new(Vec::new()),
            similarity_threshold,
            max_entries,
        }
    }

    pub async fn get(&self, query: &str) -> Option<Value> {
        let query_embedding = self.embedder.embed(query).await.ok()?;
        let entries = self.entries.read();
        let mut best_score = 0.0_f32;
        let mut best_idx = None;

        for (idx, entry) in entries.iter().enumerate() {
            let score = cosine_similarity(&query_embedding, &entry.embedding);
            if score > best_score {
                best_score = score;
                best_idx = Some(idx);
            }
        }

        if best_score >= self.similarity_threshold {
            best_idx.map(|idx| entries[idx].response.clone())
        } else {
            None
        }
    }

    pub async fn put(&self, key: &str, response: Value) {
        if let Ok(embedding) = self.embedder.embed(key).await {
            let mut entries = self.entries.write();
            if entries.len() >= self.max_entries {
                entries.remove(0);
            }
            entries.push(CacheEntry {
                embedding,
                response,
                key: key.to_string(),
            });
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::embeddings::MockEmbedder;

    #[tokio::test]
    async fn test_cache_miss_on_empty() {
        let cache = SemanticCache::new(Arc::new(MockEmbedder), 0.9, 100);
        let result = cache.get("test query").await;
        assert!(result.is_none(), "Empty cache should return None");
    }

    #[tokio::test]
    async fn test_cache_hit_after_put() {
        let cache = SemanticCache::new(Arc::new(MockEmbedder), 0.0, 100);
        cache.put("test query", serde_json::json!("cached response")).await;
        let result = cache.get("test query").await;
        assert!(result.is_some(), "Should find cached response");
        assert_eq!(result.unwrap(), serde_json::json!("cached response"));
    }

    #[tokio::test]
    async fn test_cache_eviction() {
        let cache = SemanticCache::new(Arc::new(MockEmbedder), 0.0, 2);
        cache.put("key1", serde_json::json!("r1")).await;
        cache.put("key2", serde_json::json!("r2")).await;
        cache.put("key3", serde_json::json!("r3")).await;
        assert_eq!(cache.len(), 2, "Should evict oldest entry leaving newest 2");
    }
}
