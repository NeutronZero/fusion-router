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

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "fusion_router=debug,tower_http=debug".into()),
        )
        .init();

    let config_path = std::env::var("FUSION_CONFIG")
        .unwrap_or_else(|_| "config/default.yaml".to_string());

    let config = AppConfig::load(&config_path)
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "failed to load config, using defaults");
            AppConfig::load("config/default.yaml").unwrap_or_else(|_| {
                panic!("Could not load config from config/default.yaml");
            })
        });

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

    let rate_limiter = {
        let limiter = middleware::rate_limit::RateLimiter::new(config.rate_limiting.clone());
        limiter.start_cleanup();
        limiter
    };

    let state = server::handlers::AppState::new(
        provider_router,
        resource_manager,
        Arc::new(evidence_repo),
        config,
    );

    let app = Router::new()
        .route("/v1/chat/completions", post(server::handlers::chat_completions))
        .route("/metrics", get(server::handlers::metrics_handler))
        .route("/health", get(server::health::health_handler))
        .route("/ready", get(server::health::ready_handler))
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(middleware::rate_limit::rate_limit_middleware))
        .layer(axum::Extension(rate_limiter))
        .layer(crate::middleware::cors::cors_layer_from_config(&cors_config))
        .with_state(state);

    let addr = format!("{}:{}", host, port)
        .parse::<std::net::SocketAddr>()
        .unwrap();
    tracing::info!("FusionRouter listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
