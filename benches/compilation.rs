use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use fusion_router::compiler::{build_compiler, Compiler};
use fusion_router::resource::DefaultResourceManager;
use fusion_router::types::{
    IREdge, IRMetadata, IRNode, IRNodeKind, NanoUSD, Quota, StrategyKind, WorkflowIR,
};

fn build_large_ir(node_count: usize) -> WorkflowIR {
    let nodes: Vec<IRNode> = (0..node_count)
        .map(|i| {
            let mut config = HashMap::new();
            config.insert("prompt".to_string(), serde_json::json!("test"));
            config.insert("max_tokens".to_string(), serde_json::json!(100));
            config.insert("temperature".to_string(), serde_json::json!(0.7));
            IRNode {
                id: Uuid::new_v4(),
                kind: if i % 5 == 0 {
                    IRNodeKind::Gate
                } else {
                    IRNodeKind::Generate
                },
                strategy: StrategyKind::Single,
                model: Some("gpt-4".to_string()),
                config,
            }
        })
        .collect();
    let edges: Vec<IREdge> = (0..node_count.saturating_sub(1))
        .map(|i| IREdge {
            from: nodes[i].id,
            to: nodes[i + 1].id,
            condition: None,
        })
        .collect();
    WorkflowIR {
        plan_id: Uuid::new_v4(),
        nodes,
        edges,
        metadata: IRMetadata {
            policy_applied: vec![],
            policy_version: 0,
            estimated_cost: NanoUSD::from_micros((node_count * 10) as u64).unwrap_or(NanoUSD::ZERO),
            estimated_tokens: node_count as u64 * 100,
        },
    }
}

fn bench_compilation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let quota = Quota {
        max_daily_cost: NanoUSD::from_nanos(1_000_000_000_000_000),
        max_daily_tokens: 1_000_000_000,
        max_concurrent: 100,
        provider_limits: Default::default(),
    };
    let resource_manager = Arc::new(DefaultResourceManager::new(quota));

    let compiler = build_compiler(Default::default(), resource_manager, None);

    c.bench_function("compile_10_nodes", |b| {
        let ir = build_large_ir(10);
        b.to_async(&rt)
            .iter(|| compiler.compile(black_box(ir.clone())));
    });

    c.bench_function("compile_100_nodes", |b| {
        let ir = build_large_ir(100);
        b.to_async(&rt)
            .iter(|| compiler.compile(black_box(ir.clone())));
    });

    c.bench_function("compile_500_nodes", |b| {
        let ir = build_large_ir(500);
        b.to_async(&rt)
            .iter(|| compiler.compile(black_box(ir.clone())));
    });
}

criterion_group!(benches, bench_compilation);
criterion_main!(benches);
