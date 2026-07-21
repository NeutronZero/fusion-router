pub mod embeddings;

#[cfg(feature = "semantic-cache")]
mod semantic_cache;

#[cfg(feature = "semantic-cache")]
pub use semantic_cache::SemanticCache;
