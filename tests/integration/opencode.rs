use std::path::PathBuf;
use std::sync::Arc;

use axum::{routing::get, routing::post, Router};
use tower_http::trace::TraceLayer;

use fusion_router::config::AppConfig;
use fusion_router::providers::ChatProvider;
use fusion_router::resource::DefaultResourceManager;
use fusion_router::telemetry::EvidenceRepository;
use fusion_router::types::{ChatCompletionRequest, NanoUSD, Quota};

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
            native_tool_calls: None,
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
    async fn get_model_stats(&self, _window_hours: u32) -> anyhow::Result<Vec<fusion_router::telemetry::ModelPerformanceStats>> {
        Ok(vec![])
    }
}

/// Provider that emits genuine incremental SSE chunks upstream.
struct StreamingMockProvider;

#[async_trait::async_trait]
impl ChatProvider for StreamingMockProvider {
    fn name(&self) -> &str { "stream-mock" }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> anyhow::Result<fusion_router::types::ChatCompletionResponse> {
        Ok(fusion_router::types::ChatCompletionResponse {
            id: "stream-mock-id".to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: request.model.clone(),
            choices: vec![fusion_router::types::Choice {
                index: 0,
                message: fusion_router::types::ChatMessage {
                    role: "assistant".to_string(),
                    content: "Hello from stream!".to_string(),
                },
                finish_reason: "stop".to_string(),
            }],
            native_tool_calls: None,
            usage: Some(fusion_router::types::Usage {
                prompt_tokens: 1, completion_tokens: 4, total_tokens: 5,
            }),
        })
    }

    async fn chat_stream(
        &self,
        _request: &ChatCompletionRequest,
    ) -> anyhow::Result<futures::stream::BoxStream<'static, anyhow::Result<fusion_router::types::ChatStreamChunk>>> {
        use fusion_router::types::ChatStreamChunk;
        let pieces = ["Hello ", "from ", "stream!"];
        let s = futures::stream::unfold(0usize, move |i| async move {
            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            if i < pieces.len() {
                Some((
                    Ok(ChatStreamChunk {
                        content: Some(pieces[i].to_string()),
                        finish_reason: None,
                        usage: None,
                    }),
                    i + 1,
                ))
            } else if i == pieces.len() {
                Some((
                    Ok(ChatStreamChunk {
                        content: None,
                        finish_reason: Some("stop".into()),
                        usage: Some(fusion_router::types::Usage {
                            prompt_tokens: 1, completion_tokens: 4, total_tokens: 5,
                        }),
                    }),
                    i + 1,
                ))
            } else {
                None
            }
        });
        Ok(Box::pin(s))
    }
}

/// Behavioral streaming check: stream=true on a single-node graph must hit the
/// upstream incrementally (native mode), not re-chunk a completed response.
#[tokio::test]
async fn test_native_streaming_end_to_end() {
    let target = fusion_router::providers::router::ProviderTarget::new(
        "stream-mock".into(),
        fusion_router::providers::circuit_breaker::CircuitBreaker::new(3, 2, 5),
        Box::new(|| Arc::new(StreamingMockProvider) as Arc<dyn ChatProvider + Send + Sync>),
    );
    let registry = Arc::new(fusion_router::providers::registry::ProviderRegistry::new(target));

    let provider = Arc::new(StreamingMockProvider);
    let resource_manager = DefaultResourceManager::new(Quota {
        max_daily_cost: NanoUSD::from_nanos(100_000_000_000),
        max_daily_tokens: 100000,
        max_concurrent: 10,
        provider_limits: Default::default(),
    });
    let evidence: Arc<dyn EvidenceRepository + Send + Sync> = Arc::new(NoopEvidence);
    let config = AppConfig::load("config/default.yaml").unwrap_or_else(|_| test_config());

    let state = fusion_router::server::handlers::AppState::new(
        provider,
        resource_manager,
        evidence,
        config.clone(),
        PathBuf::from("config/default.yaml"),
        Arc::new(fusion_router::scheduler::connector_resolver::ConnectorResolver::new()),
    )
    .with_provider_registry(registry);

    let app = Router::new()
        .route("/v1/chat/completions", post(fusion_router::server::handlers::chat_completions))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });

    let client = reqwest::Client::new();
    let started = std::time::Instant::now();
    let resp = client
        .post(format!("http://{addr}/v1/chat/completions"))
        .json(&serde_json::json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("x-fusion-stream-mode").and_then(|v| v.to_str().ok()),
        Some("native"),
        "single-node graph must stream natively"
    );
    let body = resp.text().await.unwrap();
    // Three upstream chunks must arrive as separate SSE events (plus usage and DONE)
    assert!(body.contains("Hello"), "first chunk content missing");
    assert!(body.contains("stream!"), "last content chunk missing");
    assert!(body.contains("[DONE]"), "SSE terminator missing");
    let data_events = body.matches("data:").count();
    assert!(data_events >= 5, "expected >=5 SSE events (3 chunks + usage + DONE), got {data_events}");
    // Upstream sleeps 15ms x 4 => total must reflect real streaming, not instant replay
    let elapsed = started.elapsed();
    assert!(
        elapsed >= std::time::Duration::from_millis(45),
        "native stream should take at least chunk pacing time, took {elapsed:?}"
    );
}

#[tokio::test]
async fn test_chat_completion_endpoint() {
    let provider = Arc::new(MockProvider);
    let resource_manager = DefaultResourceManager::new(Quota {
        max_daily_cost: NanoUSD::from_nanos(100_000_000_000),
        max_daily_tokens: 100000,
        max_concurrent: 10,
        provider_limits: Default::default(),
    });
    let evidence: Arc<dyn EvidenceRepository + Send + Sync> = Arc::new(NoopEvidence);
    let config = AppConfig::load("config/default.yaml").unwrap_or_else(|_| {
        AppConfig {
            unsafe_dev: false,
            server: fusion_router::config::ServerConfig { host: "0.0.0.0".to_string(), port: 8080, shutdown_timeout_secs: 30, request_timeout_secs: 300, cors: Default::default() },
            resources: fusion_router::config::ResourceConfig {
                max_daily_cost: fusion_router::types::NanoUSD::from_nanos(100_000_000_000),
                max_daily_tokens: 100000,
                max_concurrent: 10,
                max_concurrent_nodes: 16,
                provider_limits: Default::default(),
            },
            policies: vec![],
            providers: Default::default(),
            strategies: fusion_router::config::StrategyConfig { consensus_count: 3 },
            tools: Default::default(),
            auth: Default::default(),
            rate_limiting: Default::default(),
            logging: Default::default(),
            model_catalog: Default::default(),
            connectors: std::collections::HashMap::new(),
            features: std::collections::HashMap::new(),
        }
    });

    let state = fusion_router::server::handlers::AppState::new(
        provider,
        resource_manager,
        evidence,
        config,
        PathBuf::from("config/default.yaml"),
        Arc::new(fusion_router::scheduler::connector_resolver::ConnectorResolver::new()),
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
    assert!(body["choices"][0]["message"]["content"].as_str().unwrap().contains("Hello from mock"));
}

/// Behavioral Law 2/5 check: a policy created at runtime through the admin
/// API must deny compilation of matching chat requests — no restart, no
/// audit-only downgrade.
#[tokio::test]
async fn test_runtime_deny_policy_blocks_chat_completions() {
    use fusion_router::operations::policy_admin::PolicyAdmin;
    use fusion_router::policy::PolicyDeclaration;
    use fusion_router::telemetry::audit::AuditLog;

    let provider = Arc::new(MockProvider);
    let resource_manager = DefaultResourceManager::new(Quota {
        max_daily_cost: NanoUSD::from_nanos(100_000_000_000),
        max_daily_tokens: 100000,
        max_concurrent: 10,
        provider_limits: Default::default(),
    });
    let evidence: Arc<dyn EvidenceRepository + Send + Sync> = Arc::new(NoopEvidence);
    let config = AppConfig::load("config/default.yaml")
        .unwrap_or_else(|_| test_config());

    let state = fusion_router::server::handlers::AppState::new(
        provider,
        resource_manager,
        evidence,
        config.clone(),
        PathBuf::from("config/default.yaml"),
        Arc::new(fusion_router::scheduler::connector_resolver::ConnectorResolver::new()),
    );
    let admin = PolicyAdmin::new(state.policy_registry.clone(), Arc::new(AuditLog::new(100)));

    let app = Router::new()
        .route("/v1/chat/completions", post(fusion_router::server::handlers::chat_completions))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });

    let client = reqwest::Client::new();
    let url = format!("http://{addr}/v1/chat/completions");
    let payload = serde_json::json!({
        "model": "test-model",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    // Baseline: no policies → success.
    let resp = client.post(&url).json(&payload).send().await.unwrap();
    assert_eq!(resp.status(), 200, "request must succeed before any policy exists");

    // Operator creates a wildcard deny at runtime.
    admin
        .create_policy(PolicyDeclaration {
            name: "deny-everything".into(),
            priority: 100,
            match_target: "*".into(),
            effect: "deny".into(),
            conditions: Default::default(),
            annotations: Default::default(),
        })
        .expect("deny policy must be accepted");

    // Same request must now be rejected with the policy denial surfaced.
    let resp = client.post(&url).json(&payload).send().await.unwrap();
    let status = resp.status();
    assert!(
        status.is_client_error() || status.is_server_error(),
        "denied request must not succeed, got {status}"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    let content = body["choices"][0]["message"]["content"].as_str().unwrap_or_default();
    assert!(
        content.contains("deny-everything") || content.contains("policy"),
        "error must cite the denying rule, got: {content}"
    );

    // Removing the policy restores service without a restart.
    admin.delete_policy("deny-everything").unwrap();
    let resp = client.post(&url).json(&payload).send().await.unwrap();
    assert_eq!(resp.status(), 200, "service must recover after policy removal");
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
    let scheduler = DefaultScheduler::new(16);

    let split_id = Uuid::new_v4();
    let a_id = Uuid::new_v4();
    let b_id = Uuid::new_v4();
    let join_id = Uuid::new_v4();
    let final_id = Uuid::new_v4();

    let graph = ExecutionGraph {
        graph_id: Uuid::nil(),
        primitive_graph_hash: 0,
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
                subgraph: None,
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
                subgraph: None,
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
                subgraph: None,
            },
            ExecutionNode {
                id: join_id, kind: ExecutionNodeKind::Join,
                strategy: StrategyKind::Single, model: "test".into(),
                retry_policy: RetryPolicy { max_retries: 0, backoff_ms: 0 },
                fallback: None,
                config: HashMap::new(),
                subgraph: None,
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
                subgraph: None,
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
            policy_version: 0,
            estimated_cost: NanoUSD::from_nanos(30_000_000),
            estimated_tokens: 1500,
            max_depth: 3,
            node_count: 5,
        },
        total_tokens: 1500,
        total_cost: NanoUSD::from_nanos(1_000_000_000),
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

use fusion_router::config::{AuthConfig, CorsConfig, RateLimitingConfig, LoggingConfig, ServerConfig, ResourceConfig, StrategyConfig, ToolsConfig};
use fusion_router::middleware;

struct MidMockProvider;

#[async_trait::async_trait]
impl ChatProvider for MidMockProvider {
    fn name(&self) -> &str { "mock" }
    async fn chat_completion(
        &self,
        request: &fusion_router::types::ChatCompletionRequest,
    ) -> anyhow::Result<fusion_router::types::ChatCompletionResponse> {
        Ok(fusion_router::types::ChatCompletionResponse {
            id: "mock-id".into(),
            object: "chat.completion".into(),
            created: chrono::Utc::now().timestamp(),
            model: request.model.clone(),
            choices: vec![fusion_router::types::Choice {
                index: 0,
                message: fusion_router::types::ChatMessage { role: "assistant".into(), content: "Hello!".into() },
                finish_reason: "stop".into(),
            }],
            native_tool_calls: None,
            usage: Some(fusion_router::types::Usage { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 }),
        })
    }
}

fn test_config() -> AppConfig {
    AppConfig {
        unsafe_dev: false,
        server: ServerConfig { host: "0.0.0.0".into(), port: 8080, shutdown_timeout_secs: 30, request_timeout_secs: 300, cors: CorsConfig::default() },
        resources: ResourceConfig { max_daily_cost: fusion_router::types::NanoUSD::from_nanos(100_000_000_000), max_daily_tokens: 100000, max_concurrent: 10, max_concurrent_nodes: 16, provider_limits: Default::default() },
        policies: vec![], providers: Default::default(),
        strategies: StrategyConfig { consensus_count: 3 }, tools: ToolsConfig::default(),
        auth: AuthConfig { enabled: true, api_keys: vec!["test-key:operator".into(), "chat-key".into()] },
        rate_limiting: RateLimitingConfig::default(),
        logging: LoggingConfig::default(),
        model_catalog: Default::default(),
        connectors: std::collections::HashMap::new(),
        features: std::collections::HashMap::new(),
    }
}

#[tokio::test]
async fn test_middleware_stack_rejects_unauthenticated() {
    let provider = Arc::new(MidMockProvider);
    let resource_manager = DefaultResourceManager::new(Quota {
        max_daily_cost: NanoUSD::from_nanos(100_000_000_000), max_daily_tokens: 100000, max_concurrent: 10, provider_limits: Default::default(),
    });
    let evidence: Arc<dyn EvidenceRepository + Send + Sync> = Arc::new(NoopEvidence);
    let config = test_config();

    let state = fusion_router::server::handlers::AppState::new(provider, resource_manager, evidence, config.clone(), PathBuf::from("config/default.yaml"), Arc::new(fusion_router::scheduler::connector_resolver::ConnectorResolver::new()));

    let rate_limiter = middleware::rate_limit::RateLimiter::new(config.rate_limiting.clone());
    let app = Router::new()
        .route("/v1/chat/completions", post(fusion_router::server::handlers::chat_completions))
        .route("/health", get(fusion_router::server::health::health_handler))
        .route("/metrics", get(fusion_router::server::handlers::metrics_handler))
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(middleware::rate_limit::rate_limit_middleware))
        .layer(axum::Extension(rate_limiter))
        .layer(axum::middleware::from_fn(middleware::request_id::request_id_middleware))
        .layer(axum::middleware::from_fn(middleware::auth::auth_middleware))
        .layer(axum::Extension(fusion_router::middleware::auth::AuthHandle::from_config(&config.auth)))
        .layer(fusion_router::middleware::cors::cors_layer_from_config(&config.server.cors))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });

    let client = reqwest::Client::new();

    // Without auth key -> 401
    let res = client.post(format!("http://{}/v1/chat/completions", addr))
        .json(&serde_json::json!({"model":"test","messages":[{"role":"user","content":"hi"}]}))
        .send().await.unwrap();
    assert_eq!(res.status(), 401);

    // With valid key -> 200
    let res = client.post(format!("http://{}/v1/chat/completions", addr))
        .header("x-api-key", "test-key")
        .json(&serde_json::json!({"model":"test","messages":[{"role":"user","content":"hi"}]}))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);

    // Health endpoint is whitelisted -> 200
    let res = client.get(format!("http://{}/health", addr)).send().await.unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn test_middleware_request_id_header() {
    let provider = Arc::new(MidMockProvider);
    let resource_manager = DefaultResourceManager::new(Quota {
        max_daily_cost: NanoUSD::from_nanos(100_000_000_000), max_daily_tokens: 100000, max_concurrent: 10, provider_limits: Default::default(),
    });
    let evidence: Arc<dyn EvidenceRepository + Send + Sync> = Arc::new(NoopEvidence);
    let config = test_config();

    let state = fusion_router::server::handlers::AppState::new(provider, resource_manager, evidence, config.clone(), PathBuf::from("config/default.yaml"), Arc::new(fusion_router::scheduler::connector_resolver::ConnectorResolver::new()));

    let rate_limiter = middleware::rate_limit::RateLimiter::new(config.rate_limiting.clone());
    let app = Router::new()
        .route("/health", get(fusion_router::server::health::health_handler))
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(middleware::rate_limit::rate_limit_middleware))
        .layer(axum::Extension(rate_limiter))
        .layer(axum::middleware::from_fn(middleware::request_id::request_id_middleware))
        .layer(axum::middleware::from_fn(middleware::auth::auth_middleware))
        .layer(axum::Extension(fusion_router::middleware::auth::AuthHandle::from_config(&config.auth)))
        .layer(fusion_router::middleware::cors::cors_layer_from_config(&config.server.cors))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });

    let client = reqwest::Client::new();

    // Generated request ID
    let res = client.get(format!("http://{}/health", addr)).send().await.unwrap();
    assert!(res.headers().contains_key("x-request-id"));

    // Passthrough request ID
    let res = client.get(format!("http://{}/health", addr))
        .header("x-request-id", "my-custom-id")
        .send().await.unwrap();
    assert_eq!(res.headers().get("x-request-id").unwrap(), "my-custom-id");
}

#[tokio::test]
async fn test_config_validation_valid() {
    let config = test_config();
    assert!(config.validate().is_ok());
}

#[tokio::test]
async fn test_config_validation_auth_no_keys() {
    let mut config = test_config();
    config.auth.enabled = true;
    config.auth.api_keys.clear();
    assert!(config.validate().is_err());
}

#[tokio::test]
async fn test_config_validation_invalid_format() {
    let mut config = test_config();
    config.logging.format = "xml".into();
    assert!(config.validate().is_err());
}

#[tokio::test]
async fn test_health_ready_endpoints() {
    let provider = Arc::new(MidMockProvider);
    let resource_manager = DefaultResourceManager::new(Quota {
        max_daily_cost: NanoUSD::from_nanos(100_000_000_000), max_daily_tokens: 100000, max_concurrent: 10, provider_limits: Default::default(),
    });
    let evidence: Arc<dyn EvidenceRepository + Send + Sync> = Arc::new(NoopEvidence);
    let config = test_config();

    let state = fusion_router::server::handlers::AppState::new(provider, resource_manager, evidence, config.clone(), PathBuf::from("config/default.yaml"), Arc::new(fusion_router::scheduler::connector_resolver::ConnectorResolver::new()));

    let rate_limiter = middleware::rate_limit::RateLimiter::new(config.rate_limiting.clone());
    let app = Router::new()
        .route("/health", get(fusion_router::server::health::health_handler))
        .route("/ready", get(fusion_router::server::health::ready_handler))
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(middleware::rate_limit::rate_limit_middleware))
        .layer(axum::Extension(rate_limiter))
        .layer(axum::middleware::from_fn(middleware::request_id::request_id_middleware))
        .layer(axum::middleware::from_fn(middleware::auth::auth_middleware))
        .layer(axum::Extension(fusion_router::middleware::auth::AuthHandle::from_config(&config.auth)))
        .layer(fusion_router::middleware::cors::cors_layer_from_config(&config.server.cors))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });

    let client = reqwest::Client::new();
    let res = client.get(format!("http://{}/health", addr)).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["status"], "ok");

    let res = client.get(format!("http://{}/ready", addr)).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn test_executions_and_operations_auth_enforcement() {
    use std::collections::HashMap;
    use fusion_router::events::BroadcastEventBus;
    use fusion_router::executor::DefaultExecutor;
    use fusion_router::operations::{
        attestation_viewer::AttestationViewer, dashboard::DefaultDashboardDataProvider,
        handlers::OperationsState, policy_admin::PolicyAdmin, runtime_inspector::RuntimeInspector,
        MockPackageVerifier, RuntimeModuleCache,
    };
    use fusion_router::server::execution::{build_execution_plane, execute_workflow_handler};
    use fusion_router::telemetry::audit::AuditLog;

    let provider = Arc::new(MidMockProvider);
    let resource_manager = DefaultResourceManager::new(Quota {
        max_daily_cost: NanoUSD::from_nanos(100_000_000_000),
        max_daily_tokens: 100000,
        max_concurrent: 10,
        provider_limits: Default::default(),
    });
    let evidence: Arc<dyn EvidenceRepository + Send + Sync> = Arc::new(NoopEvidence);
    let config = test_config();

    let state = fusion_router::server::handlers::AppState::new(
        provider.clone(),
        resource_manager,
        evidence,
        config.clone(),
        PathBuf::from("config/default.yaml"),
        Arc::new(fusion_router::scheduler::connector_resolver::ConnectorResolver::new()),
    );

    let ops_cache = Arc::new(RuntimeModuleCache::new());
    let ops_dashboard = Arc::new(DefaultDashboardDataProvider::new(state.capability_registry.clone(), ops_cache.clone()));
    let ops_inspector = Arc::new(RuntimeInspector::new(ops_cache));
    let ops_audit = Arc::new(AuditLog::new(1000));
    let ops_policy_admin = Arc::new(PolicyAdmin::new(state.policy_registry.clone(), ops_audit.clone()));
    let ops_verifier = Arc::new(MockPackageVerifier);
    let ops_attestation_viewer = Arc::new(AttestationViewer::new(ops_verifier, ops_audit));

    let ops_state = OperationsState {
        dashboard: ops_dashboard,
        inspector: ops_inspector,
        policy_admin: ops_policy_admin,
        attestation_viewer: ops_attestation_viewer,
    };

    let operations_routes = Router::new()
        .route("/v1/operations/registry", get(fusion_router::operations::handlers::registry_handler))
        .with_state(ops_state);

    let event_bus = Arc::new(BroadcastEventBus::new(64));
    let executor = Arc::new(DefaultExecutor::new(provider, HashMap::new()));
    let exec_plane = build_execution_plane(
        event_bus,
        executor,
        config.model_catalog.clone(),
        state.resource_manager.clone(),
        state.policy_registry.clone(),
    );
    let execution_routes = Router::new()
        .route("/v1/executions", post(execute_workflow_handler))
        .with_state(exec_plane);

    let app = Router::new()
        .route("/v1/chat/completions", post(fusion_router::server::handlers::chat_completions))
        .with_state(state)
        .merge(operations_routes)
        .merge(execution_routes)
        .layer(axum::middleware::from_fn(middleware::auth::auth_middleware))
        .layer(axum::Extension(fusion_router::middleware::auth::AuthHandle::from_config(&config.auth)));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();

    // 1. Unauthenticated POST /v1/executions -> 401 Unauthorized
    let res = client
        .post(format!("http://{}/v1/executions", addr))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);

    // 2. Unauthenticated GET /v1/operations/registry -> 401 Unauthorized
    let res = client
        .get(format!("http://{}/v1/operations/registry", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);

    // 3. Valid API key POST /v1/executions -> passes auth middleware
    let res = client
        .post(format!("http://{}/v1/executions", addr))
        .header("x-api-key", "test-key")
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_ne!(res.status(), reqwest::StatusCode::UNAUTHORIZED);

    // 4. Valid operator key GET /v1/operations/registry -> 200 OK
    let res = client
        .get(format!("http://{}/v1/operations/registry", addr))
        .header("x-api-key", "test-key")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);

    // 5. A chat-scoped key is authenticated but forbidden on operator routes.
    let res = client
        .get(format!("http://{}/v1/operations/registry", addr))
        .header("x-api-key", "chat-key")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);
}




