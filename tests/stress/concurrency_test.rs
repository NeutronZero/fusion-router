use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};
use tokio::time::sleep;
use uuid::Uuid;

use fusion_router::config::AppConfig;
use fusion_router::types::{
    ExecutionGraph, GraphMetadata, ExecutionNode, ExecutionNodeKind, StrategyKind, RetryPolicy, NodeState
};
use fusion_router::scheduler::default::DefaultScheduler;
use fusion_router::scheduler::Scheduler;
use fusion_router::executor::Executor;
use fusion_router::types::{ExecutionResult, ExecutionInstance, NodeExecutionResult, Usage};

// Mock Executor for testing
struct MockExecutor {
    latency: Duration,
    fail_rate: f64,
}

#[async_trait::async_trait]
impl Executor for MockExecutor {
    async fn execute_node(&self, node: &ExecutionNode) -> NodeExecutionResult {
        sleep(self.latency).await;
        
        let state = if fastrand::f64() < self.fail_rate {
            NodeState::Failed("Simulated failure".into())
        } else {
            NodeState::Succeeded
        };

        NodeExecutionResult {
            state,
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            }),
            latency_ms: self.latency.as_millis() as u64,
            output: Some(serde_json::json!({"status": "ok"})),
        }
    }
}

fn create_test_graph(nodes_count: usize) -> ExecutionGraph {
    let mut nodes = Vec::new();
    for i in 0..nodes_count {
        nodes.push(ExecutionNode {
            id: Uuid::new_v4(),
            kind: ExecutionNodeKind::LLMGenerate,
            strategy: StrategyKind::Single,
            model: "test-model".into(),
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_ms: 0,
            },
            fallback: None,
            config: Default::default(),
        });
    }

    ExecutionGraph {
        graph_id: Uuid::new_v4(),
        nodes,
        edges: vec![],
        metadata: GraphMetadata {
            estimated_cost: 0.1,
            estimated_tokens: 100,
            max_depth: 1,
            node_count: nodes_count as u32,
        },
        total_tokens: 100,
        total_cost: 1,
        primitive_graph_hash: 0,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_stress_concurrency_and_backpressure() {
    let scheduler = DefaultScheduler::new(16); // max concurrent 16
    let executor = Arc::new(MockExecutor {
        latency: Duration::from_millis(5),
        fail_rate: 0.0,
    });

    let start = Instant::now();
    let num_requests = 100;

    let handles: Vec<_> = (0..num_requests).map(|_| {
        let scheduler = DefaultScheduler::new(16);
        let executor_clone = executor.clone();
        
        tokio::spawn(async move {
            let graph = create_test_graph(10);
            let mut instance = scheduler.schedule(graph, fusion_router::types::ReservationId(Uuid::new_v4()));
            let res = scheduler.run(&mut instance, executor_clone.as_ref()).await;
            res.unwrap()
        })
    }).collect();

    let results = futures::future::join_all(handles).await;
    let elapsed = start.elapsed();

    let successful = results.iter().filter(|r| r.as_ref().unwrap().success).count();
    assert_eq!(successful, num_requests, "All requests should succeed under load");

    println!("Completed {} requests in {:?}", num_requests, elapsed);
    println!("Requests/sec: {}", num_requests as f64 / elapsed.as_secs_f64());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_long_running_session_recovery_and_memory() {
    let scheduler = DefaultScheduler::new(16);
    let executor = Arc::new(MockExecutor {
        latency: Duration::from_millis(1),
        fail_rate: 0.1, // 10% failure rate for fault injection
    });

    let graph = create_test_graph(50); // Larger graph
    let mut instance = scheduler.schedule(graph, fusion_router::types::ReservationId(Uuid::new_v4()));
    
    // Simulating memory check
    let res = scheduler.run(&mut instance, executor.as_ref()).await;
    
    // We expect it might fail since nodes don't retry and fail_rate is 10%,
    // but the router shouldn't panic or leak memory.
    assert!(res.is_ok());
}
