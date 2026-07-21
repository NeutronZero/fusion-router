use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use fusion_router::compiler::passes::{
    BudgetOptimisationPass, ConstraintValidationPass, ControlFlowValidationPass, ModelResolutionPass,
};
use fusion_router::compiler::{Compiler, DefaultCompiler};
use fusion_router::resource::DefaultResourceManager;
use fusion_router::types::{
    IRMetadata, IRNode, IRNodeKind, IREdge, Quota, StrategyKind, WorkflowIR,
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
                kind: if i % 5 == 0 { IRNodeKind::Gate } else { IRNodeKind::Generate },
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
            estimated_cost: node_count as f64 * 0.01,
            estimated_tokens: node_count as u64 * 100,
        },
    }
}

fn bench_compilation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let quota = Quota {
        max_daily_cost: 1_000_000.0,
        max_daily_tokens: 1_000_000_000,
        max_concurrent: 100,
        provider_limits: Default::default(),
    };
    let resource_manager = Arc::new(DefaultResourceManager::new(quota));

    let compiler = DefaultCompiler {
        passes: vec![
            Box::new(ConstraintValidationPass),
            Box::new(ControlFlowValidationPass),
            Box::new(BudgetOptimisationPass { resource_manager }),
            Box::new(ModelResolutionPass { model_catalog: Default::default() }),
        ],
    };

    c.bench_function("compile_10_nodes", |b| {
        let ir = build_large_ir(10);
        b.to_async(&rt).iter(|| compiler.compile(black_box(ir.clone())));
    });

    c.bench_function("compile_100_nodes", |b| {
        let ir = build_large_ir(100);
        b.to_async(&rt).iter(|| compiler.compile(black_box(ir.clone())));
    });

    c.bench_function("compile_500_nodes", |b| {
        let ir = build_large_ir(500);
        b.to_async(&rt).iter(|| compiler.compile(black_box(ir.clone())));
    });
}

criterion_group!(benches, bench_compilation);
criterion_main!(benches);
