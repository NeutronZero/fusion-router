#![allow(dead_code)]
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
mod workflow;
mod tools;
mod cache;
mod middleware;

use config::AppConfig;
use providers::openrouter::OpenRouterProvider;
use providers::router::ProviderRouter;
use providers::zen::ZenProvider;
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

    let zen_key = std::env::var("OPENCODEZEN_API_KEY")
        .unwrap_or_else(|_| "test-key".to_string());
    let openrouter_key = std::env::var("OPENROUTER_API_KEY")
        .unwrap_or_else(|_| "test-key".to_string());

    let zen_provider = Arc::new(ZenProvider::new(zen_key));
    let openrouter_provider = Arc::new(OpenRouterProvider::new(openrouter_key));

    let provider_router: Arc<dyn providers::ChatProvider + Send + Sync> = Arc::new(
        ProviderRouter::new(openrouter_provider.clone())
            .with_provider(
                vec!["opencode/".to_string(), "zen/".to_string()],
                zen_provider,
            ),
    );

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

    let state = server::handlers::AppState::new(
        provider_router,
        resource_manager,
        Arc::new(evidence_repo),
        config,
    );

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
