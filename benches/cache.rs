use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use std::sync::Arc;

use fusion_router::cache::embeddings::MockEmbedder;
use fusion_router::cache::SemanticCache;

fn bench_cache_hits(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let cache = Arc::new(SemanticCache::new(Arc::new(MockEmbedder), 0.9, 10_000, 384));

    for i in 0..1000 {
        rt.block_on(cache.put(
            &format!("key-{}", i),
            serde_json::json!(format!("response-{}", i)),
        ));
    }

    let mut group = c.benchmark_group("cache_hits");
    group.throughput(Throughput::Elements(1));

    group.bench_function("hnsw_hit", |b| {
        b.to_async(&rt).iter(|| async {
            let _ = black_box(cache.get("key-42").await);
        });
    });

    group.bench_function("hnsw_miss", |b| {
        b.to_async(&rt).iter(|| async {
            let _ = black_box(cache.get("nonexistent-key").await);
        });
    });

    group.finish();
}

fn bench_cache_concurrent(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let cache = Arc::new(SemanticCache::new(Arc::new(MockEmbedder), 0.9, 10_000, 384));

    let mut group = c.benchmark_group("cache_concurrent");
    group.throughput(Throughput::Elements(100));
    group.sample_size(10);

    group.bench_function("hnsw_100_concurrent", |b| {
        b.to_async(&rt).iter(|| async {
            let mut handles = Vec::new();
            for i in 0..100 {
                let c = cache.clone();
                handles.push(tokio::spawn(async move {
                    let _ = c.get(&format!("key-{}", i)).await;
                }));
            }
            for h in handles {
                let _ = h.await;
            }
        });
    });
    group.finish();
}

fn bench_eviction(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let cache = Arc::new(SemanticCache::new(Arc::new(MockEmbedder), 0.9, 100, 384));

    c.bench_function("cache_eviction_100_entries", |b| {
        b.to_async(&rt).iter(|| async {
            for i in 0..200 {
                cache
                    .put(&format!("key-{}", i), serde_json::json!(format!("val-{}", i)))
                    .await;
            }
        });
    });
}

criterion_group!(cache_benches, bench_cache_hits, bench_cache_concurrent, bench_eviction);
criterion_main!(cache_benches);
