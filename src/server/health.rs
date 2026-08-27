use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};

use crate::server::handlers::AppState;

pub async fn health_handler() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

pub async fn ready_handler(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    // Real readiness signals only — no hardcoded "ok" placeholders.
    let db_ok = state.evidence_repository.ping().await;
    let providers_configured = state
        .provider_registry
        .as_ref()
        .map(|r| r.target_count() > 0)
        .unwrap_or(true);

    let checks = json!({
        "database": if db_ok { "ok" } else { "unavailable" },
        "providers": if providers_configured { "ok" } else { "none-configured" },
    });

    if db_ok && providers_configured {
        (
            StatusCode::OK,
            Json(json!({"status": "ok", "checks": checks})),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "unavailable", "checks": checks})),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AppConfig, AuthConfig, CorsConfig, LoggingConfig, RateLimitingConfig, ResourceConfig,
        ServerConfig, StrategyConfig, ToolsConfig,
    };
    use crate::scheduler::connector_resolver::ConnectorResolver;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn dummy_state() -> AppState {
        let config = AppConfig {
            unsafe_dev: false,
            server: ServerConfig {
                host: "0.0.0.0".into(),
                port: 0,
                shutdown_timeout_secs: 30,
                request_timeout_secs: 300,
                cors: CorsConfig::default(),
            },
            resources: ResourceConfig {
                max_daily_cost: crate::types::NanoUSD::from_nanos(100_000_000_000),
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
            connectors: HashMap::new(),
            features: HashMap::new(),
            streaming: Default::default(),
        };
        crate::server::handlers::AppState::new(
            Arc::new(crate::providers::openrouter::OpenRouterProvider::new(
                "test".into(),
            )),
            crate::resource::DefaultResourceManager::new(config.to_quota()),
            Arc::new(crate::telemetry::SqliteEvidenceRepository::new(":memory:").unwrap()),
            config,
            PathBuf::from("config/default.yaml"),
            Arc::new(ConnectorResolver::new()),
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
