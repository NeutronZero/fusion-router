use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{routing::post, Router};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use fusion_router::config::AppConfig;
use fusion_router::providers::ChatProvider;
use fusion_router::resource::DefaultResourceManager;
use fusion_router::telemetry::EvidenceRepository;
use fusion_router::types::{ChatCompletionRequest, Quota};

struct LoadMockProvider;

#[async_trait::async_trait]
impl ChatProvider for LoadMockProvider {
    fn name(&self) -> &str { "load-mock" }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<fusion_router::types::ChatCompletionResponse> {
        tokio::time::sleep(Duration::from_millis(5)).await;
        Ok(fusion_router::types::ChatCompletionResponse {
            id: "load-id".to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: request.model.clone(),
            choices: vec![fusion_router::types::Choice {
                index: 0,
                message: fusion_router::types::ChatMessage {
                    role: "assistant".to_string(),
                    content: "load test response".to_string(),
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Some(fusion_router::types::Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        })
    }
}

struct NoopEvidence;

#[async_trait::async_trait]
impl EvidenceRepository for NoopEvidence {
    async fn record(&self, _entry: fusion_router::types::ExecutionRecord) -> anyhow::Result<()> { Ok(()) }
    async fn snapshot(&self) -> anyhow::Result<fusion_router::types::EvidenceSnapshot> {
        Ok(fusion_router::types::EvidenceSnapshot {
            record_count: 0, success_rates: Default::default(),
            avg_latencies: Default::default(), avg_costs: Default::default(),
            model_rankings: vec![],
        })
    }
}

fn build_app(quota: &Quota) -> Router {
    let provider: Arc<dyn ChatProvider + Send + Sync> = Arc::new(LoadMockProvider);
    let resource_manager = DefaultResourceManager::new(quota.clone());
    let evidence: Arc<dyn EvidenceRepository + Send + Sync> = Arc::new(NoopEvidence);
    let config = AppConfig {
        server: fusion_router::config::ServerConfig { host: "0.0.0.0".to_string(), port: 0 },
        resources: fusion_router::config::ResourceConfig {
            max_daily_cost: quota.max_daily_cost,
            max_daily_tokens: quota.max_daily_tokens,
            max_concurrent: quota.max_concurrent,
            provider_limits: Default::default(),
        },
        policies: vec![],
        providers: Default::default(),
        strategies: fusion_router::config::StrategyConfig { consensus_count: 3 },
        tools: Default::default(),
    };

    let state = fusion_router::server::handlers::AppState::new(provider, resource_manager, evidence, config);
    Router::new()
        .route("/v1/chat/completions", post(fusion_router::server::handlers::chat_completions))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

fn make_request_body() -> serde_json::Value {
    serde_json::json!({
        "model": "test-model",
        "messages": [{"role": "user", "content": "Hello from load test"}]
    })
}

#[tokio::test]
async fn test_concurrent_throughput() {
    let quota = Quota {
        max_daily_cost: 1000.0,
        max_daily_tokens: 1_000_000,
        max_concurrent: 100,
        provider_limits: Default::default(),
    };
    let app = build_app(&quota);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });

    tokio::time::sleep(Duration::from_millis(100)).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();

    let concurrency = [1, 10, 50];

    for &n in &concurrency {
        let start = Instant::now();
        let mut handles = Vec::new();

        for _ in 0..n {
            let client = client.clone();
            let url = format!("http://{}/v1/chat/completions", addr);
            let body = make_request_body();
            handles.push(tokio::spawn(async move {
                let t0 = Instant::now();
                let resp = client.post(&url).json(&body).send().await;
                let latency = t0.elapsed();
                (resp, latency)
            }));
        }

        let mut ok = 0u32;
        let mut err = 0u32;
        let mut latencies = Vec::new();
        for h in handles {
            if let Ok((Ok(resp), lat)) = h.await {
                if resp.status().is_success() { ok += 1; } else { err += 1; }
                latencies.push(lat);
            } else {
                err += 1;
            }
        }

        let elapsed = start.elapsed();
        latencies.sort();
        let p50 = latencies.get(latencies.len() / 2).map(|d| d.as_millis()).unwrap_or(0);
        let p90 = latencies.get((latencies.len() as f64 * 0.9) as usize).map(|d| d.as_millis()).unwrap_or(0);
        let p99 = latencies.get((latencies.len() as f64 * 0.99) as usize).map(|d| d.as_millis()).unwrap_or(0);

        println!(
            "concurrency={}: {} ok / {} err in {:?} | p50={}ms p90={}ms p99={}ms throughput={:.0} req/s",
            n, ok, err, elapsed, p50, p90, p99, ok as f64 / elapsed.as_secs_f64()
        );

        assert!(ok >= n - 1, "Expected ~{} success, got {}", n, ok);
    }
}

#[tokio::test]
async fn test_quota_enforcement() {
    let quota = Quota {
        max_daily_cost: 0.001,
        max_daily_tokens: 10,
        max_concurrent: 100,
        provider_limits: Default::default(),
    };
    let app = build_app(&quota);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });

    tokio::time::sleep(Duration::from_millis(100)).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let mut successes = 0u32;
    let mut rejections = 0u32;

    for _ in 0..20 {
        let resp = client
            .post(format!("http://{}/v1/chat/completions", addr))
            .json(&make_request_body())
            .send()
            .await
            .unwrap();
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap();
        let is_error = body["choices"][0]["finish_reason"].as_str() == Some("error");
        if status.is_success() && !is_error {
            successes += 1;
        } else {
            rejections += 1;
        }
    }

    println!("quota test: {} success / {} rejected", successes, rejections);
    assert!(rejections > 0, "Quota exhaustion should produce rejections");
    assert!(successes < 20, "Budget-limited requests should fail after exhaust");
}

#[tokio::test]
async fn test_concurrent_streaming() {
    let quota = Quota {
        max_daily_cost: 1000.0,
        max_daily_tokens: 1_000_000,
        max_concurrent: 50,
        provider_limits: Default::default(),
    };
    let app = build_app(&quota);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });

    tokio::time::sleep(Duration::from_millis(100)).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();

    let n = 20;
    let start = Instant::now();
    let mut handles = Vec::new();

    for _ in 0..n {
        let client = client.clone();
        let url = format!("http://{}/v1/chat/completions", addr);
        let body = serde_json::json!({
            "model": "test-model",
            "stream": true,
            "messages": [{"role": "user", "content": "Count to 3"}]
        });
        handles.push(tokio::spawn(async move {
            let t0 = Instant::now();
            let resp = client.post(&url).json(&body).send().await;
            let latency = t0.elapsed();
            let (status_ok, body_text) = match resp {
                Ok(r) => {
                    let status = r.status().is_success();
                    let body = r.text().await.ok();
                    (status, body)
                }
                Err(_) => (false, None),
            };
            (status_ok, latency, body_text)
        }));
    }

    let mut ok = 0u32;
    let mut err = 0u32;
    for h in handles {
        if let Ok((status_ok, _lat, body)) = h.await {
            if status_ok { ok += 1; } else { err += 1; }
            if let Some(text) = body {
                assert!(text.contains("[DONE]"), "Streaming response should contain [DONE]");
            }
        } else {
            err += 1;
        }
    }

    let elapsed = start.elapsed();
    println!(
        "streaming concurrency={}: {} ok / {} err in {:?}",
        n, ok, err, elapsed
    );
    assert_eq!(err, 0, "All streaming requests should succeed");
}

#[tokio::test]
async fn test_concurrent_dag_workflows() {
    use std::collections::HashMap;
    use fusion_router::executor::DefaultExecutor;
    use fusion_router::scheduler::default::DefaultScheduler;
    use fusion_router::scheduler::Scheduler;

    use fusion_router::strategies::single::SingleStrategy;
    use fusion_router::strategies::Strategy;
    use fusion_router::types::{
        ExecutionEdge, ExecutionGraph, ExecutionNode, ExecutionNodeKind,
        GraphMetadata, RetryPolicy, StrategyKind, ReservationId,
    };

    let provider: Arc<dyn ChatProvider + Send + Sync> = Arc::new(LoadMockProvider);
    let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
    strategies.insert(StrategyKind::Single, Box::new(SingleStrategy));
    let executor = Arc::new(DefaultExecutor::new(provider, strategies));
    let scheduler = Arc::new(DefaultScheduler::new());

    let split_id = Uuid::new_v4();
    let a_id = Uuid::new_v4();
    let b_id = Uuid::new_v4();
    let join_id = Uuid::new_v4();

    fn make_node(id: Uuid, kind: ExecutionNodeKind) -> ExecutionNode {
        let mut config = HashMap::new();
        config.insert("messages".into(), serde_json::json!([{"role": "user", "content": "hello"}]));
        ExecutionNode {
            id, kind, strategy: StrategyKind::Single, model: "test".into(),
            retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
            fallback: None, config,
        }
    }

    let graph = ExecutionGraph {
        graph_id: Uuid::nil(),
        nodes: vec![
            make_node(split_id, ExecutionNodeKind::Split),
            make_node(a_id, ExecutionNodeKind::LLMGenerate),
            make_node(b_id, ExecutionNodeKind::LLMGenerate),
            make_node(join_id, ExecutionNodeKind::Join),
        ],
        edges: vec![
            ExecutionEdge { from: split_id, to: a_id, condition: None },
            ExecutionEdge { from: split_id, to: b_id, condition: None },
            ExecutionEdge { from: a_id, to: join_id, condition: None },
            ExecutionEdge { from: b_id, to: join_id, condition: None },
        ],
        metadata: GraphMetadata {
            estimated_cost: 0.02, estimated_tokens: 1000, max_depth: 2, node_count: 4,
        },
        total_tokens: 1000,
        total_cost: 1,
    };

    let start = Instant::now();
    let n = 100;
    let mut handles = Vec::new();

    for _ in 0..n {
        let graph = graph.clone();
        let scheduler = scheduler.clone();
        let executor = executor.clone();
        handles.push(tokio::spawn(async move {
            let mut instance = scheduler.schedule(graph, ReservationId(Uuid::new_v4()));
            scheduler.run(&mut instance, &*executor).await
        }));
    }

    let mut ok = 0u32;
    let mut err = 0u32;
    for h in handles {
        match h.await {
            Ok(Ok(ref result)) if result.success => ok += 1,
            _ => err += 1,
        }
    }

    let elapsed = start.elapsed();
    println!(
        "concurrent DAG: {} ok / {} err in {:?} throughput={:.0} graphs/s",
        ok, err, elapsed, n as f64 / elapsed.as_secs_f64()
    );
    assert_eq!(err, 0, "All concurrent DAG workflows should succeed");
}

#[tokio::test]
async fn test_loop_iteration_stress() {
    use std::collections::HashMap;
    use fusion_router::executor::DefaultExecutor;
    use fusion_router::scheduler::default::DefaultScheduler;
    use fusion_router::scheduler::Scheduler;

    use fusion_router::strategies::single::SingleStrategy;
    use fusion_router::strategies::Strategy;
    use fusion_router::types::{
        ExecutionEdge, ExecutionGraph, ExecutionNode, ExecutionNodeKind,
        GraphMetadata, RetryPolicy, StrategyKind, ReservationId,
    };

    let provider: Arc<dyn ChatProvider + Send + Sync> = Arc::new(LoadMockProvider);
    let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
    strategies.insert(StrategyKind::Single, Box::new(SingleStrategy));
    let executor = Arc::new(DefaultExecutor::new(provider, strategies));
    let scheduler = Arc::new(DefaultScheduler::new());

    let loop_id = Uuid::new_v4();
    let body_id = Uuid::new_v4();
    let exit_id = Uuid::new_v4();

    let mut loop_config = HashMap::new();
    loop_config.insert("max_iterations".into(), serde_json::json!(50));

    let mut body_config = HashMap::new();
    body_config.insert("messages".into(), serde_json::json!([{"role": "user", "content": "iter"}]));

    let graph = ExecutionGraph {
        graph_id: Uuid::nil(),
        nodes: vec![
            ExecutionNode {
                id: loop_id, kind: ExecutionNodeKind::Loop,
                strategy: StrategyKind::Single, model: "test".into(),
                retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                fallback: None, config: loop_config,
            },
            ExecutionNode {
                id: body_id, kind: ExecutionNodeKind::LLMGenerate,
                strategy: StrategyKind::Single, model: "test".into(),
                retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                fallback: None, config: body_config,
            },
            ExecutionNode {
                id: exit_id, kind: ExecutionNodeKind::LLMGenerate,
                strategy: StrategyKind::Single, model: "test".into(),
                retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                fallback: None, config: HashMap::new(),
            },
        ],
        edges: vec![
            ExecutionEdge { from: loop_id, to: body_id, condition: None },
            ExecutionEdge { from: body_id, to: loop_id, condition: Some("loop".into()) },
            ExecutionEdge { from: loop_id, to: exit_id, condition: Some("exit".into()) },
        ],
        metadata: GraphMetadata {
            estimated_cost: 0.5, estimated_tokens: 25000, max_depth: 50, node_count: 3,
        },
        total_tokens: 25000,
        total_cost: 1,
    };

    let mut instance = scheduler.schedule(graph, ReservationId(Uuid::new_v4()));
    instance.outputs.insert(loop_id, serde_json::json!(true));

    let start = Instant::now();
    let result = scheduler.run(&mut instance, &*executor).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "Loop should complete");
    assert!(result.unwrap().success, "All nodes should succeed");
    println!("loop 50 iterations completed in {:?}", elapsed);
}
