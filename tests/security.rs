use axum::{
    extract::Request,
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Router,
};

use fusion_router::config::AuthConfig;
use fusion_router::middleware::auth::auth_middleware;
use fusion_router::tools::builtin::FileReadTool;
use fusion_router::tools::ShellCommandTool;
use fusion_router::tools::Tool;

#[tokio::test]
async fn test_api_key_bruteforce() {
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn(auth_middleware))
        .layer(axum::Extension(AuthConfig {
            enabled: true,
            api_keys: vec!["valid-key".into()],
        }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    for key in &["wrong-key", "another-key", "hack-key", "", "invalid"] {
        let res = client
            .get(format!("http://{}/", addr))
            .header("x-api-key", *key)
            .send()
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "key {:?} should be rejected",
            key
        );
    }
}

#[tokio::test]
async fn test_v1_executions_auth_enforcement() {
    use fusion_router::events::BroadcastEventBus;
    use fusion_router::executor::DefaultExecutor;
    use fusion_router::providers::ChatProvider;
    use fusion_router::resource::DefaultResourceManager;
    use fusion_router::server::execution::{build_execution_plane, execute_workflow_handler};
    use std::collections::HashMap;
    use std::sync::Arc;

    struct DummyProvider;
    #[async_trait::async_trait]
    impl ChatProvider for DummyProvider {
        fn name(&self) -> &str {
            "dummy"
        }
        async fn chat_completion(
            &self,
            _req: &fusion_router::types::ChatCompletionRequest,
        ) -> anyhow::Result<fusion_router::types::ChatCompletionResponse> {
            anyhow::bail!("not implemented")
        }
    }

    let event_bus = Arc::new(BroadcastEventBus::new(64));
    let executor = Arc::new(DefaultExecutor::new(Arc::new(DummyProvider), HashMap::new()));
    let plane_compiler: Arc<dyn fusion_router::compiler::Compiler> = Arc::new(
        fusion_router::compiler::build_compiler(
            fusion_router::types::ModelCatalog::default(),
            Arc::new(DefaultResourceManager::new(fusion_router::types::Quota {
                max_daily_cost: fusion_router::types::NanoUSD::from_nanos(1_000_000_000_000),
                max_daily_tokens: 1_000_000_000,
                max_concurrent: 100,
                provider_limits: Default::default(),
            })),
            None,
        ),
    );
    let exec_plane = build_execution_plane(event_bus, executor, plane_compiler);
    let execution_routes = Router::new()
        .route("/v1/executions", post(execute_workflow_handler))
        .with_state(exec_plane);

    let app = Router::new()
        .merge(execution_routes)
        .layer(axum::middleware::from_fn(auth_middleware))
        .layer(axum::Extension(AuthConfig {
            enabled: true,
            api_keys: vec!["valid-key".into()],
        }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();

    // Unauthenticated request to POST /v1/executions -> 401 Unauthorized
    let res = client
        .post(format!("http://{}/v1/executions", addr))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Invalid API key -> 401 Unauthorized
    let res = client
        .post(format!("http://{}/v1/executions", addr))
        .header("x-api-key", "wrong-key")
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Valid API key -> Passes auth middleware (returns HTTP 400 Bad Request due to invalid workflow payload, not 401)
    let res = client
        .post(format!("http://{}/v1/executions", addr))
        .header("x-api-key", "valid-key")
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_ne!(res.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_v1_operations_auth_enforcement() {
    use fusion_router::capability::InMemoryCapabilityRegistry;
    use fusion_router::operations::{
        attestation_viewer::AttestationViewer, dashboard::DefaultDashboardDataProvider,
        handlers::OperationsState, policy_admin::PolicyAdmin, runtime_inspector::RuntimeInspector,
        MockPackageVerifier, RuntimeModuleCache,
    };
    use fusion_router::telemetry::audit::AuditLog;
    use std::sync::Arc;

    let ops_registry = Arc::new(parking_lot::RwLock::new(InMemoryCapabilityRegistry::new()));
    let ops_cache = Arc::new(RuntimeModuleCache::new());
    let ops_dashboard = Arc::new(DefaultDashboardDataProvider::new(
        ops_registry.clone(),
        ops_cache.clone(),
    ));
    let ops_inspector = Arc::new(RuntimeInspector::new(ops_cache.clone()));
    let ops_policy_registry = Arc::new(fusion_router::policy::PolicyRegistry::new());
    let ops_audit = Arc::new(AuditLog::new(1000));
    let ops_policy_admin = Arc::new(PolicyAdmin::new(ops_policy_registry, ops_audit.clone()));
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
        .route("/v1/operations/runtime", get(fusion_router::operations::handlers::runtime_handler))
        .route("/v1/operations/metrics", get(fusion_router::operations::handlers::metrics_handler))
        .route("/v1/operations/policies", get(fusion_router::operations::handlers::policies_list_handler))
        .route("/v1/operations/policies", post(fusion_router::operations::handlers::policies_create_handler))
        .route("/v1/operations/attestations", get(fusion_router::operations::handlers::attestations_handler))
        .with_state(ops_state);

    let app = Router::new()
        .merge(operations_routes)
        .layer(axum::middleware::from_fn(auth_middleware))
        .layer(axum::Extension(AuthConfig {
            enabled: true,
            api_keys: vec!["valid-key".into()],
        }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();

    let ops_endpoints = [
        "/v1/operations/registry",
        "/v1/operations/runtime",
        "/v1/operations/metrics",
        "/v1/operations/policies",
        "/v1/operations/attestations",
    ];

    for endpoint in &ops_endpoints {
        // Unauthenticated -> 401 Unauthorized
        let res = client
            .get(format!("http://{}{}", addr, endpoint))
            .send()
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "endpoint {} should reject unauthenticated request",
            endpoint
        );
    }

    // Authenticated request to /v1/operations/registry -> Passes auth (200 OK)
    let res = client
        .get(format!("http://{}/v1/operations/registry", addr))
        .header("x-api-key", "valid-key")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn test_path_traversal() {
    let tmp = std::env::temp_dir();
    let tool = FileReadTool::new(tmp.to_string_lossy().to_string());
    let result = tool.execute(serde_json::json!({"path": "../../etc/passwd"})).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("Path traversal") || err.contains("not found") || err.contains("inaccessible")
    );
}

#[tokio::test]
async fn test_shell_injection() {
    let allowed = vec![
        "cmd".to_string(),
        "sh".to_string(),
        "bash".to_string(),
        "powershell".to_string(),
        "powershell.exe".to_string(),
        "pwsh".to_string(),
        "zsh".to_string(),
        "echo".to_string(),
    ];
    let tool = ShellCommandTool::new(allowed, 5, vec![".".into()], false);

    // Unallowed command string
    let result = tool
        .execute(serde_json::json!({
            "command": "cmd /c rm -rf /"
        }))
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("not in allowed list") || err.contains("strictly prohibited"));

    // Shell binaries must be rejected even if configured in allowed list
    for shell_bin in &["cmd", "cmd.exe", "sh", "bash", "powershell", "powershell.exe", "pwsh", "zsh"] {
        let res = tool
            .execute(serde_json::json!({
                "command": shell_bin,
                "args": ["-c", "echo hello"]
            }))
            .await;
        assert!(res.is_err(), "Shell binary '{}' should be rejected", shell_bin);
        let err_msg = res.unwrap_err();
        assert!(
            err_msg.contains("strictly prohibited"),
            "Expected strictly prohibited error for '{}', got: {}",
            shell_bin,
            err_msg
        );
    }

    // Allowed non-shell command passes validation
    assert!(tool.validate_command("echo").is_ok());
}

#[tokio::test]
async fn test_oversized_payload() {
    async fn limit_body_size(
        req: Request,
        next: Next,
    ) -> Result<Response, (axum::http::StatusCode, String)> {
        const MAX_SIZE: usize = 1024;
        let content_length = req
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        if content_length > MAX_SIZE {
            return Err((
                axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                "Payload too large".to_string(),
            ));
        }
        Ok(next.run(req).await)
    }

    async fn handler() -> &'static str {
        "ok"
    }

    let app = Router::new()
        .route("/", post(handler))
        .layer(middleware::from_fn(limit_body_size));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let large_body = "x".repeat(2048);
    let res = client
        .post(format!("http://{}/", addr))
        .body(large_body)
        .header("content-type", "text/plain")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
}
