#![cfg_attr(not(test), allow(dead_code))] // Intentional: stubs for future production wiring (CircuitBreakingProvider, WorkflowPlanner, DynamicPlanner)
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
mod cache;
mod middleware;
mod devex;

#[cfg(feature = "wasm-plugins")]
mod wasm;

use config::AppConfig;
use providers::circuit_breaker::CircuitBreaker;
use providers::openrouter::OpenRouterProvider;
use providers::registry::ProviderRegistry;
use providers::router::ProviderTarget;
use providers::zen::ZenProvider;
use scheduler::connector_resolver::ConnectorResolver;
use scheduler::connector_subscriber::ConnectorSubscriber;
use telemetry::SqliteEvidenceRepository;

#[tokio::main]
async fn main() {
    let _ = dotenv::dotenv();

    telemetry::tracing::init_console();
    let _ = telemetry::tracing::init_tracing();

    let config_path = std::env::var("FUSION_CONFIG")
        .unwrap_or_else(|_| "config/default.yaml".to_string());

    let config = AppConfig::load(&config_path)
        .unwrap_or_else(|e| {
            eprintln!("failed to load config: {e}, using defaults");
            AppConfig::load("config/default.yaml").unwrap_or_else(|_| {
                panic!("Could not load config from config/default.yaml");
            })
        });

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

    let default_target = ProviderTarget::new(
        "default".to_string(),
        CircuitBreaker::new(5, 3, 30),
        Box::new(|| -> Arc<dyn providers::ChatProvider + Send + Sync> {
            Arc::new(OpenRouterProvider::new(
                std::env::var("OPENROUTER_API_KEY").unwrap_or_else(|_| "test-key".to_string())
            ))
        }),
    );
    let provider_registry = Arc::new(ProviderRegistry::new(default_target));

    for (name, cfg) in &config.providers {
        let api_key = cfg.api_key_env.as_ref()
            .and_then(|var| std::env::var(var).ok())
            .unwrap_or_else(|| format!("test-key-{name}"));

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
        config,
        PathBuf::from(&config_path),
        connector_resolver.clone(),
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

    let mut app = Router::new()
        .route("/v1/chat/completions", post(server::handlers::chat_completions))
        .route("/metrics", get(server::handlers::metrics_handler))
        .route("/health", get(server::health::health_handler))
        .route("/ready", get(server::health::ready_handler))
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(middleware::request_id::request_id_middleware))
        .layer(axum::middleware::from_fn(middleware::auth::auth_middleware))
        .layer(axum::Extension(auth_config))
        .layer(crate::middleware::cors::cors_layer_from_config(&cors_config))
        .with_state(state);

    if rate_limiting_enabled {
        let limiter = middleware::rate_limit::RateLimiter::new(rate_limiting_config);
        limiter.start_cleanup();
        app = app
            .layer(axum::middleware::from_fn(middleware::rate_limit::rate_limit_middleware))
            .layer(axum::Extension(limiter));
    }

    let addr = format!("{}:{}", host, port)
        .parse::<std::net::SocketAddr>()
        .unwrap();
    tracing::info!("FusionRouter listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
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
