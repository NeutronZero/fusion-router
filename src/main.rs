#![allow(dead_code)] // Intentional: binary crate exposes no public API; pub items in module tree are stubs for future production wiring (CircuitBreakingProvider, WorkflowPlanner, DynamicPlanner)
use std::path::PathBuf;
use std::sync::Arc;

use axum::{routing::get, routing::post, Router};
use tower_http::trace::TraceLayer;

mod server;
mod context;
mod requirements;
mod planner;
mod compiler;
mod scheduler;
mod executor;
mod strategies;
mod providers;
mod models;
mod transport;
mod resource;
mod telemetry;
mod types;
mod config;
mod plugin;
mod capability;
mod policy;
mod session;
mod lifecycle;
mod trigger;
mod connectors;
mod workflow;
mod tools;
mod security;
mod cache;
mod middleware;
mod release;
mod feature_gate;
mod events;

#[cfg(feature = "wasm-plugins")]
mod wasm;
mod operations;
mod review;

use config::AppConfig;
use providers::circuit_breaker::CircuitBreaker;
use providers::openrouter::OpenRouterProvider;
use providers::registry::ProviderRegistry;
use providers::router::ProviderTarget;
use providers::zen::ZenProvider;
use scheduler::connector_resolver::ConnectorResolver;
use scheduler::connector_subscriber::ConnectorSubscriber;
use telemetry::SqliteEvidenceRepository;

/// Resolves an API key from the environment, failing fast in release builds
/// instead of silently substituting placeholder credentials. The placeholder
/// escape hatch is limited to debug builds and explicit `--unsafe-dev` runs.
fn resolve_api_key(env_var: &str, placeholder: &str, unsafe_dev: bool) -> anyhow::Result<String> {
    if let Ok(key) = std::env::var(env_var) {
        if !key.trim().is_empty() {
            return Ok(key);
        }
    }
    if cfg!(debug_assertions) || unsafe_dev {
        tracing::warn!(
            env_var = %env_var,
            "API key missing; using placeholder key (debug/--unsafe-dev only)"
        );
        return Ok(placeholder.to_string());
    }
    anyhow::bail!(
        "API key environment variable '{env_var}' is required but missing or empty"
    )
}

#[tokio::main]
async fn main() {
    let _ = dotenv::dotenv();

    let unsafe_dev = std::env::args().any(|a| a == "--unsafe-dev");
    if unsafe_dev {
        tracing::warn!(
            "==================================================================\n\
             == WARNING: running with --unsafe-dev                          ==\n\
             == Authentication, rate limiting, CORS, and tool defaults are  ==\n\
             == NOT enforced. DO NOT expose this server to untrusted        ==\n\
             == networks.                                                   ==\n\
             =================================================================="
        );
    }

    // Offline mode: `fusion-router review [args]` runs a multi-model code
    // review entirely in-process (no HTTP server, no lifecycle binding).
    if std::env::args().nth(1).as_deref() == Some("review") {
        let env_filter = tracing_subscriber::EnvFilter::default()
            .add_directive("info".parse().expect("invalid log level"));
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
        let args = review::ReviewArgs::from_args();
        if let Err(e) = review::run(args).await {
            eprintln!("review failed: {e:?}");
            std::process::exit(1);
        }
        return;
    }

    telemetry::tracing::init_console();
    let _ = telemetry::tracing::init_tracing();

    let config_path = std::env::var("FUSION_CONFIG")
        .unwrap_or_else(|_| "config/default.yaml".to_string());

    let mut config = AppConfig::load(&config_path)
        .unwrap_or_else(|e| {
            eprintln!("failed to load config: {e}, using defaults");
            AppConfig::load("config/default.yaml").unwrap_or_else(|_| {
                panic!("Could not load config from config/default.yaml");
            })
        });

    if config.unsafe_dev && !unsafe_dev {
        // ADR-035: the escape hatch must be explicit at invocation time. A
        // config file with `unsafe_dev: true` must not silently disable
        // auth/rate-limiting/tool guards in production deployments.
        eprintln!(
            "config sets `unsafe_dev: true`, but the flag is only honored from\n\
             the command line. Start with `--unsafe-dev` if an insecure run\n\
             is intentionally required."
        );
        std::process::exit(1);
    }

    if unsafe_dev {
        config.unsafe_dev = true;
    }

    if let Err(errors) = config.validate() {
        for err in &errors {
            eprintln!("config validation error: {err}");
        }
        panic!("configuration validation failed with {} error(s)", errors.len());
    }

    let log_level = &config.logging.level;
    let log_format = &config.logging.format;

    let env_filter = tracing_subscriber::EnvFilter::default()
        .add_directive(log_level.parse().expect("invalid log level"));

    if log_format == "json" {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .init();
    }

    tracing::info!("loaded config from {}", config_path);

    let openrouter_key = resolve_api_key("OPENROUTER_API_KEY", "test-key", unsafe_dev)
        .unwrap_or_else(|e| panic!("{e}"));
    let default_target = ProviderTarget::new(
        "default".to_string(),
        CircuitBreaker::new(5, 3, 30),
        Box::new(move || -> Arc<dyn providers::ChatProvider + Send + Sync> {
            Arc::new(OpenRouterProvider::new(openrouter_key.clone()))
        }),
    );
    let provider_registry = Arc::new(ProviderRegistry::new(default_target));

    for (name, cfg) in &config.providers {
        let api_key = match cfg.api_key_env.as_ref() {
            Some(var) => resolve_api_key(var, &format!("test-key-{name}"), unsafe_dev)
                .unwrap_or_else(|e| panic!("{e}")),
            None if cfg!(debug_assertions) || unsafe_dev => {
                tracing::warn!(
                    provider = %name,
                    "no api_key_env configured; using placeholder key (debug/--unsafe-dev only)"
                );
                format!("test-key-{name}")
            }
            None => panic!(
                "provider '{name}' has no api_key_env configured; refusing to run without a credential"
            ),
        };

        let circuit_breaker = CircuitBreaker::new(
            cfg.failure_threshold,
            3,
            cfg.cooldown_secs,
        );

        let factory_name = name.clone();
        let factory_key = api_key.clone();
        let target = ProviderTarget::new(
            name.clone(),
            circuit_breaker,
            Box::new(move || -> Arc<dyn providers::ChatProvider + Send + Sync> {
                if factory_name == "openrouter" {
                    Arc::new(OpenRouterProvider::new(factory_key.clone()))
                } else {
                    Arc::new(ZenProvider::new(factory_key.clone()))
                }
            }),
        );

        provider_registry.register_target(vec![name.clone() + "/"], target);
    }

    tracing::info!(
        "providers configured: opencode-zen={}, openrouter={}",
        std::env::var("OPENCODEZEN_API_KEY").is_ok(),
        std::env::var("OPENROUTER_API_KEY").is_ok(),
    );

    let resource_manager = resource::DefaultResourceManager::new(config.to_quota());

    let evidence_repo = SqliteEvidenceRepository::new("fusion_telemetry.db")
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "failed to open telemetry db, using no-op");
            SqliteEvidenceRepository::new(":memory:").expect("in-memory db")
        });

    let host = config.server.host.clone();
    let port = config.server.port;
    let cors_config = config.server.cors.clone();
    let auth_config = config.auth.clone();
    let rate_limiting_enabled = config.rate_limiting.enabled;
    let rate_limiting_config = config.rate_limiting.clone();

    let connector_resolver = Arc::new(ConnectorResolver::new());

    let state = server::handlers::AppState::new(
        provider_registry.clone() as Arc<dyn providers::ChatProvider + Send + Sync>,
        resource_manager,
        Arc::new(evidence_repo),
        config.clone(),
        PathBuf::from(&config_path),
        connector_resolver.clone(),
    );

    // Law 5 / ADR-034: the execution plane compiles through the same
    // `build_compiler` pipeline as the chat path, with a budget pass reading
    // the shared resource manager instance (no empty pass list, no bypass).
    let exec_plane_compiler: Arc<dyn crate::compiler::Compiler> = Arc::new(
        crate::compiler::build_compiler(
            config.model_catalog.clone(),
            state.resource_manager.clone(),
            None,
        ),
    );

    state.config_manager.register_subscriber(Box::new(provider_registry.clone()));
    state.config_manager.register_subscriber(Box::new(
        ConnectorSubscriber::new(ConnectorResolver::clone(&connector_resolver)),
    ));

    #[cfg(unix)]
    {
        let cm_for_reload = state.config_manager.clone();
        tokio::spawn(async move {
            reload_signal(cm_for_reload).await;
        });
    }

    // Spawn connector health checker
    let health_checker = Arc::new(scheduler::connector_health::ConnectorHealthChecker::new(60));
    let hc_resolver = state.connector_resolver.clone();
    let hc_checker = health_checker.clone();
    tokio::spawn(async move {
        hc_checker.run(hc_resolver).await;
    });

    let ops_registry = Arc::new(parking_lot::RwLock::new(crate::capability::InMemoryCapabilityRegistry::new()));
    let ops_cache = Arc::new(crate::operations::RuntimeModuleCache::new());
    let ops_dashboard = Arc::new(crate::operations::dashboard::DefaultDashboardDataProvider::new(
        ops_registry.clone(),
        ops_cache.clone(),
    ));
    let ops_inspector = Arc::new(crate::operations::runtime_inspector::RuntimeInspector::new(ops_cache.clone()));
    let ops_store = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let ops_audit = Arc::new(crate::telemetry::audit::AuditLog::new(1000));
    let ops_policy_admin = Arc::new(crate::operations::policy_admin::PolicyAdmin::new(ops_store, ops_audit.clone()));
    let ops_verifier = Arc::new(crate::operations::MockPackageVerifier);
    let ops_attestation_viewer = Arc::new(crate::operations::attestation_viewer::AttestationViewer::new(ops_verifier, ops_audit));

    let ops_state = crate::operations::handlers::OperationsState {
        dashboard: ops_dashboard,
        inspector: ops_inspector,
        policy_admin: ops_policy_admin,
        attestation_viewer: ops_attestation_viewer,
    };

    let operations_routes = axum::Router::new()
        .route("/v1/operations/registry", axum::routing::get(crate::operations::handlers::registry_handler))
        .route("/v1/operations/runtime", axum::routing::get(crate::operations::handlers::runtime_handler))
        .route("/v1/operations/metrics", axum::routing::get(crate::operations::handlers::metrics_handler))
        .route("/v1/operations/policies", axum::routing::get(crate::operations::handlers::policies_list_handler))
        .route("/v1/operations/policies", axum::routing::post(crate::operations::handlers::policies_create_handler))
        .route("/v1/operations/attestations", axum::routing::get(crate::operations::handlers::attestations_handler))
        .with_state(ops_state);

    let event_bus = Arc::new(crate::events::BroadcastEventBus::new(1024));
    let exec_plane = crate::server::execution::build_execution_plane(
        event_bus,
        state.executor.clone(),
        exec_plane_compiler,
    );
    let execution_routes = axum::Router::new()
        .route("/v1/executions", axum::routing::post(crate::server::execution::execute_workflow_handler))
        .with_state(exec_plane);

    let mut app = Router::new()
        .route("/v1/chat/completions", post(server::handlers::chat_completions))
        .route("/v1/messages", post(server::handlers::anthropic_messages))
        .route("/metrics", get(server::handlers::metrics_handler))
        .route("/health", get(server::health::health_handler))
        .route("/ready", get(server::health::ready_handler))
        .with_state(state.clone())
        .merge(operations_routes)
        .merge(execution_routes)
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(middleware::request_id::request_id_middleware));

    // ADR-035: the rate limiter sits INSIDE the auth layer so it keys on the
    // authenticated identity (set via ClientIdentity) rather than spoofable
    // headers; unauthenticated traffic falls back to the TCP peer address.
    if rate_limiting_enabled {
        let limiter = middleware::rate_limit::RateLimiter::new(rate_limiting_config);
        limiter.start_cleanup();
        app = app
            .layer(axum::middleware::from_fn(middleware::rate_limit::rate_limit_middleware))
            .layer(axum::Extension(limiter));
    }

    let app = app
        .layer(axum::middleware::from_fn(middleware::auth::auth_middleware))
        .layer(axum::Extension(auth_config))
        .layer(crate::middleware::cors::cors_layer_from_config(&cors_config));

    let addr = format!("{}:{}", host, port)
        .parse::<std::net::SocketAddr>()
        .unwrap();
    tracing::info!("FusionRouter listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    let shutdown_timeout = std::time::Duration::from_secs(config.server.shutdown_timeout_secs);

    // ADR: `shutdown_timeout_secs` bounds graceful drain; without it a stuck
    // in-flight request can hang shutdown indefinitely.
    let serve = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal());

    match tokio::time::timeout(shutdown_timeout, serve).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            eprintln!("server error: {e}");
            std::process::exit(1);
        }
        Err(_) => {
            tracing::warn!(
                timeout_secs = %config.server.shutdown_timeout_secs,
                "graceful shutdown exceeded its bound; forcing exit"
            );
        }
    }
}

#[cfg(unix)]
async fn reload_signal(config_manager: Arc<config::manager::ConfigManager>) {
    use tokio::signal::unix;
    let mut stream = unix::signal(unix::SignalKind::hangup())
        .expect("failed to install SIGHUP handler");

    while stream.recv().await.is_some() {
        match config_manager.reload().await {
            Ok(gen) => tracing::info!(generation = gen, "configuration reloaded"),
            Err(e) => tracing::error!(error = %e, "reload failed, continuing with previous config"),
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received, gracefully shutting down");
}



