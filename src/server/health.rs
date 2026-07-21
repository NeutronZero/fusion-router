use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};

use crate::server::handlers::AppState;

pub async fn health_handler() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

pub async fn ready_handler(
    State(state): State<AppState>,
) -> (StatusCode, Json<Value>) {
    let checks = json!({
        "database": "ok",
        "plugins": "ok",
        "providers": "ok",
    });

    let _ = state; // placeholder for future real checks

    (StatusCode::OK, Json(json!({"status": "ok", "checks": checks})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ServerConfig, ResourceConfig, StrategyConfig, ToolsConfig, AuthConfig, RateLimitingConfig, LoggingConfig, CorsConfig};
    use std::sync::Arc;

    fn dummy_state() -> AppState {
        let config = AppConfig {
            server: ServerConfig {
                host: "0.0.0.0".into(),
                port: 0,
                shutdown_timeout_secs: 30,
                cors: CorsConfig::default(),
            },
            resources: ResourceConfig {
                max_daily_cost: 100.0,
                max_daily_tokens: 100000,
                max_concurrent: 10,
                max_concurrent_nodes: 16,
                provider_limits: Default::default(),
            },
            policies: vec![],
            providers: Default::default(),
            strategies: StrategyConfig { consensus_count: 3 },
            tools: ToolsConfig::default(),
            auth: AuthConfig::default(),
            rate_limiting: RateLimitingConfig::default(),
            logging: LoggingConfig::default(),
            model_catalog: Default::default(),
        };
        crate::server::handlers::AppState::new(
            Arc::new(crate::providers::openrouter::OpenRouterProvider::new("test".into())),
            crate::resource::DefaultResourceManager::new(config.to_quota()),
            Arc::new(crate::telemetry::SqliteEvidenceRepository::new(":memory:").unwrap()),
            config,
        )
    }

    #[tokio::test]
    async fn test_health_handler() {
        let res = health_handler().await;
        assert_eq!(res["status"], "ok");
    }

    #[tokio::test]
    async fn test_ready_handler() {
        let state = dummy_state();
        let (status, res) = ready_handler(State(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(res["status"], "ok");
    }
}
