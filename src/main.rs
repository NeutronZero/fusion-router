use axum::{routing::post, Router};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
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

#[tokio::main]
async fn main() {
    let _ = dotenv::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "fusion_router=debug,tower_http=debug".into()),
        )
        .init();

    let zen_key = std::env::var("OPENCODEZEN_API_KEY")
        .unwrap_or_else(|_| "test-key".to_string());
    let openrouter_key = std::env::var("OPENROUTER_API_KEY")
        .unwrap_or_else(|_| "test-key".to_string());

    let zen_provider = Arc::new(providers::zen::ZenProvider::new(zen_key));
    let openrouter_provider = Arc::new(providers::openrouter::OpenRouterProvider::new(openrouter_key));

    let router = providers::router::ProviderRouter::new(openrouter_provider.clone())
        .with_provider(
            vec!["opencode/".to_string(), "zen/".to_string()],
            zen_provider,
        );

    tracing::info!(
        "providers configured: opencode-zen={}, openrouter={}",
        std::env::var("OPENCODEZEN_API_KEY").is_ok(),
        std::env::var("OPENROUTER_API_KEY").is_ok(),
    );

    let state = server::handlers::AppState {
        provider: Arc::new(router),
    };

    let app = Router::new()
        .route("/v1/chat/completions", post(server::handlers::chat_completions))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("FusionRouter listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
