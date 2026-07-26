use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

use fusion_router::compiler::context::CompilationContext;
use fusion_router::compiler::ir::{DebateRole, StrategyIR};
use fusion_router::strategies::chain::ChainStrategy;
use fusion_router::strategies::consensus::ConsensusStrategy;
use fusion_router::strategies::debate::DebateStrategy;
use fusion_router::strategies::fusion::FusionStrategy;
use fusion_router::strategies::react::ReActStrategy;
use fusion_router::strategies::reflection::ReflectionStrategy;
use fusion_router::strategies::single::SingleStrategy;
use fusion_router::strategies::Strategy;

fn ctx() -> CompilationContext {
    let mut c = CompilationContext::new();
    c.available_models = vec!["gpt-4".into(), "claude-opus-4".into()];
    c
}

fn bench_single(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_lowering");
    group.throughput(Throughput::Elements(1));

    let strategy = SingleStrategy;
    let ir = StrategyIR::Single;
    let context = ctx();

    group.bench_function("SingleStrategy", |b| {
        b.iter(|| {
            let g = strategy.lower(black_box(&ir), black_box(&context)).unwrap();
            black_box(g)
        });
    });

    group.finish();
}

fn bench_consensus(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_lowering");
    group.throughput(Throughput::Elements(1));
    let context = ctx();

    for count in [2u32, 3, 5] {
        let strategy = ConsensusStrategy { count };
        let ir = StrategyIR::Consensus { count };

        group.bench_function(format!("ConsensusStrategy/{}", count), |b| {
            b.iter(|| {
                let g = strategy.lower(black_box(&ir), black_box(&context)).unwrap();
                black_box(g)
            });
        });
    }

    group.finish();
}

fn bench_fusion(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_lowering");
    group.throughput(Throughput::Elements(1));
    let context = ctx();
    let ir = StrategyIR::Single;

    for n in [2usize, 3] {
        let sub: Vec<Box<dyn Strategy>> =
            (0..n).map(|_| Box::new(SingleStrategy) as Box<dyn Strategy>).collect();
        let strategy = FusionStrategy::new(sub);

        group.bench_function(format!("FusionStrategy/{}_sub", n), |b| {
            b.iter(|| {
                let g = strategy.lower(black_box(&ir), black_box(&context)).unwrap();
                black_box(g)
            });
        });
    }

    group.finish();
}

fn bench_chain(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_lowering");
    group.throughput(Throughput::Elements(1));
    let context = ctx();

    for n in [2usize, 3] {
        let stages: Vec<Box<dyn Strategy>> =
            (0..n).map(|_| Box::new(SingleStrategy) as Box<dyn Strategy>).collect();
        let strategy = ChainStrategy { stages };
        let ir = StrategyIR::Chain {
            stages: vec![StrategyIR::Single; n],
        };

        group.bench_function(format!("ChainStrategy/{}_stages", n), |b| {
            b.iter(|| {
                let g = strategy.lower(black_box(&ir), black_box(&context)).unwrap();
                black_box(g)
            });
        });
    }

    group.finish();
}

fn bench_reflection(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_lowering");
    group.throughput(Throughput::Elements(1));
    let context = ctx();

    let strategy = ReflectionStrategy::default();
    let ir = StrategyIR::Reflection { max_cycles: 3 };

    group.bench_function("ReflectionStrategy", |b| {
        b.iter(|| {
            let g = strategy.lower(black_box(&ir), black_box(&context)).unwrap();
            black_box(g)
        });
    });

    group.finish();
}

fn bench_react(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_lowering");
    group.throughput(Throughput::Elements(1));
    let context = ctx();

    let strategy = ReActStrategy::default();
    let ir = StrategyIR::ReAct { max_iterations: 10 };

    group.bench_function("ReActStrategy", |b| {
        b.iter(|| {
            let g = strategy.lower(black_box(&ir), black_box(&context)).unwrap();
            black_box(g)
        });
    });

    group.finish();
}

fn bench_debate(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_lowering");
    group.throughput(Throughput::Elements(1));
    let context = ctx();

    for n in [2usize, 3] {
        let roles: Vec<DebateRole> = (0..n)
            .map(|i| DebateRole {
                name: format!("Debater_{}", i + 1),
                model: "gpt-4".into(),
                stance: if i == 0 { "Defend".into() } else { "Critique".into() },
            })
            .collect();
        let strategy = DebateStrategy {
            debaters: vec![],
            judge: Box::new(SingleStrategy),
        };
        let ir = StrategyIR::Debate { roles };

        group.bench_function(format!("DebateStrategy/{}_debaters", n), |b| {
            b.iter(|| {
                let g = strategy.lower(black_box(&ir), black_box(&context)).unwrap();
                black_box(g)
            });
        });
    }

    group.finish();
}

criterion_group!(
    strategy_lowering,
    bench_single,
    bench_consensus,
    bench_fusion,
    bench_chain,
    bench_reflection,
    bench_react,
    bench_debate,
);
criterion_main!(strategy_lowering);
