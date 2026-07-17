use std::sync::Arc;

use axum::{routing::post, Router};
use tower_http::trace::TraceLayer;

use fusion_router::config::AppConfig;
use fusion_router::providers::ChatProvider;
use fusion_router::resource::DefaultResourceManager;
use fusion_router::telemetry::EvidenceRepository;
use fusion_router::types::{ChatCompletionRequest, Quota};

struct MockProvider;

#[async_trait::async_trait]
impl ChatProvider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<fusion_router::types::ChatCompletionResponse> {
        Ok(fusion_router::types::ChatCompletionResponse {
            id: "mock-id".to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: request.model.clone(),
            choices: vec![fusion_router::types::Choice {
                index: 0,
                message: fusion_router::types::ChatMessage {
                    role: "assistant".to_string(),
                    content: "Hello from mock!".to_string(),
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
    async fn record(&self, _entry: fusion_router::types::ExecutionRecord) -> anyhow::Result<()> {
        Ok(())
    }
    async fn snapshot(&self) -> anyhow::Result<fusion_router::types::EvidenceSnapshot> {
        Ok(fusion_router::types::EvidenceSnapshot {
            record_count: 0,
            success_rates: Default::default(),
            avg_latencies: Default::default(),
            avg_costs: Default::default(),
            model_rankings: vec![],
        })
    }
}

#[tokio::test]
async fn test_chat_completion_endpoint() {
    let provider = Arc::new(MockProvider);
    let resource_manager = DefaultResourceManager::new(Quota {
        max_daily_cost: 100.0,
        max_daily_tokens: 100000,
        max_concurrent: 10,
        provider_limits: Default::default(),
    });
    let evidence: Arc<dyn EvidenceRepository + Send + Sync> = Arc::new(NoopEvidence);
    let config = AppConfig::load("config/default.yaml").unwrap_or_else(|_| {
        AppConfig {
            server: fusion_router::config::ServerConfig { host: "0.0.0.0".to_string(), port: 8080 },
            resources: fusion_router::config::ResourceConfig {
                max_daily_cost: 100.0,
                max_daily_tokens: 100000,
                max_concurrent: 10,
                provider_limits: Default::default(),
            },
            policies: vec![],
            providers: Default::default(),
            strategies: fusion_router::config::StrategyConfig { consensus_count: 3 },
            tools: Default::default(),
        }
    });

    let state = fusion_router::server::handlers::AppState::new(
        provider,
        resource_manager,
        evidence,
        config,
    );

    let app = Router::new()
        .route("/v1/chat/completions", post(fusion_router::server::handlers::chat_completions))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/chat/completions", addr))
        .json(&serde_json::json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert!(body["choices"][0]["message"]["content"].as_str().unwrap().contains("processed successfully"));
}

#[tokio::test]
async fn test_dag_split_join_workflow() {
    use std::collections::HashMap;
    use fusion_router::executor::DefaultExecutor;
    use fusion_router::scheduler::default::DefaultScheduler;
    use fusion_router::scheduler::Scheduler;
    use fusion_router::strategies::single::SingleStrategy;
    use fusion_router::strategies::Strategy;
    use fusion_router::types::{
        ExecutionEdge, ExecutionGraph, ExecutionNode, ExecutionNodeKind,
        GraphMetadata, RetryPolicy, StrategyKind,
    };
    use uuid::Uuid;

    let provider = Arc::new(MockProvider);
    let mut strategies: HashMap<StrategyKind, Box<dyn Strategy + Send + Sync>> = HashMap::new();
    strategies.insert(StrategyKind::Single, Box::new(SingleStrategy));

    let executor = DefaultExecutor::new(provider, strategies);
    let scheduler = DefaultScheduler::new();

    let split_id = Uuid::new_v4();
    let a_id = Uuid::new_v4();
    let b_id = Uuid::new_v4();
    let join_id = Uuid::new_v4();
    let final_id = Uuid::new_v4();

    let graph = ExecutionGraph {
        graph_id: Uuid::nil(),
        nodes: vec![
            ExecutionNode {
                id: split_id, kind: ExecutionNodeKind::Split,
                strategy: StrategyKind::Single, model: "test".into(),
                retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                fallback: None,
                config: {
                    let mut m = HashMap::new();
                    m.insert("messages".into(), serde_json::json!([{"role": "user", "content": "hello"}]));
                    m
                },
            },
            ExecutionNode {
                id: a_id, kind: ExecutionNodeKind::LLMGenerate,
                strategy: StrategyKind::Single, model: "test".into(),
                retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                fallback: None,
                config: {
                    let mut m = HashMap::new();
                    m.insert("messages".into(), serde_json::json!([{"role": "user", "content": "hello"}]));
                    m
                },
            },
            ExecutionNode {
                id: b_id, kind: ExecutionNodeKind::LLMGenerate,
                strategy: StrategyKind::Single, model: "test".into(),
                retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                fallback: None,
                config: {
                    let mut m = HashMap::new();
                    m.insert("messages".into(), serde_json::json!([{"role": "user", "content": "hello"}]));
                    m
                },
            },
            ExecutionNode {
                id: join_id, kind: ExecutionNodeKind::Join,
                strategy: StrategyKind::Single, model: "test".into(),
                retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                fallback: None,
                config: HashMap::new(),
            },
            ExecutionNode {
                id: final_id, kind: ExecutionNodeKind::LLMGenerate,
                strategy: StrategyKind::Single, model: "test".into(),
                retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                fallback: None,
                config: {
                    let mut m = HashMap::new();
                    m.insert("messages".into(), serde_json::json!([{"role": "user", "content": "hello"}]));
                    m
                },
            },
        ],
        edges: vec![
            ExecutionEdge { from: split_id, to: a_id, condition: None },
            ExecutionEdge { from: split_id, to: b_id, condition: None },
            ExecutionEdge { from: a_id, to: join_id, condition: None },
            ExecutionEdge { from: b_id, to: join_id, condition: None },
            ExecutionEdge { from: join_id, to: final_id, condition: None },
        ],
        metadata: GraphMetadata {
            estimated_cost: 0.03,
            estimated_tokens: 1500,
            max_depth: 3,
            node_count: 5,
        },
        total_tokens: 1500,
        total_cost: 1,
    };

    let reservation = fusion_router::types::ReservationId(Uuid::new_v4());
    let mut instance = scheduler.schedule(graph, reservation);

    let result = scheduler.run(&mut instance, &executor).await;
    assert!(result.is_ok(), "Split/Join workflow should succeed");

    let exec_result = result.unwrap();
    assert!(exec_result.success, "DAG workflow should complete successfully");

    let succeeded: Vec<_> = instance.node_states.values()
        .filter(|s| **s == fusion_router::types::NodeState::Succeeded)
        .collect();
    assert_eq!(succeeded.len(), 5, "All 5 nodes should succeed (Split + A + B + Join + Final)");
}
