pub mod anthropic;
pub mod chat;
pub mod health;
pub mod state;

pub use anthropic::*;
pub use chat::*;
pub use health::*;
pub use state::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::connector_resolver::ConnectorResolver;
    use crate::types::*;
    use axum::http::StatusCode;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_health_endpoint() {
        let res = crate::server::health::health_handler().await;
        assert_eq!(res["status"], "ok");
    }

    #[tokio::test]
    async fn test_ready_endpoint() {
        use crate::config::{
            AppConfig, AuthConfig, CorsConfig, LoggingConfig, RateLimitingConfig, ResourceConfig,
            ServerConfig, StrategyConfig, ToolsConfig,
        };
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
        };
        let state = AppState::new(
            Arc::new(crate::providers::openrouter::OpenRouterProvider::new(
                "test".into(),
            )),
            crate::resource::DefaultResourceManager::new(config.to_quota()),
            Arc::new(crate::telemetry::SqliteEvidenceRepository::new(":memory:").unwrap()),
            config,
            PathBuf::from("config/default.yaml"),
            Arc::new(ConnectorResolver::new()),
        );
        let (status, res) = crate::server::health::ready_handler(axum::extract::State(state)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(res["status"], "ok");
    }

    #[test]
    fn test_invalid_json_returns_400() {
        let bad_json = r#"{"model": "test"}"#;
        let result: Result<ChatCompletionRequest, _> = serde_json::from_str(bad_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_response_format() {
        let request_id = Uuid::new_v4();
        let response = chat::error_response(request_id, "test-model", "something went wrong");
        assert_eq!(response.model, "test-model");
        assert_eq!(response.choices[0].finish_reason, "error");
        assert!(response.choices[0]
            .message
            .content
            .contains("something went wrong"));
        assert_eq!(response.object, "chat.completion");
    }

    #[test]
    fn test_anthropic_request_deserialization_and_conversion() {
        let json_body = serde_json::json!({
            "model": "claude-3-5-sonnet-20241022",
            "system": "You are a helpful assistant.",
            "messages": [
                {"role": "user", "content": "Hello Anthropic!"}
            ],
            "max_tokens": 512,
            "temperature": 0.7
        });

        let anthropic_req: AnthropicMessagesRequest = serde_json::from_value(json_body).unwrap();
        assert_eq!(anthropic_req.model, "claude-3-5-sonnet-20241022");

        let chat_req = anthropic_req.into_chat_completion_request();
        assert_eq!(chat_req.model, "claude-3-5-sonnet-20241022");
        assert_eq!(chat_req.messages.len(), 2);
        assert_eq!(chat_req.messages[0].role, "system");
        assert_eq!(chat_req.messages[0].content, "You are a helpful assistant.");
        assert_eq!(chat_req.messages[1].role, "user");
        assert_eq!(chat_req.messages[1].content, "Hello Anthropic!");
        assert_eq!(chat_req.max_tokens, Some(512));
    }

    #[test]
    fn test_anthropic_response_conversion() {
        let completion_resp = ChatCompletionResponse {
            id: "resp-123".into(),
            object: "chat.completion".into(),
            created: 1700000000,
            model: "claude-3-5-sonnet".into(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: "Hi from Anthropic response!".into(),
                },
                finish_reason: "stop".into(),
            }],
            native_tool_calls: None,
            usage: Some(Usage {
                prompt_tokens: 15,
                completion_tokens: 8,
                total_tokens: 23,
            }),
        };

        let anthropic_resp =
            AnthropicMessagesResponse::from((completion_resp, "claude-3-5-sonnet".to_string()));
        assert_eq!(anthropic_resp.id, "msg_resp-123");
        assert_eq!(anthropic_resp.r#type, "message");
        assert_eq!(anthropic_resp.role, "assistant");
        assert_eq!(anthropic_resp.stop_reason, Some("end_turn".into()));
        assert_eq!(anthropic_resp.usage.input_tokens, 15);
        assert_eq!(anthropic_resp.usage.output_tokens, 8);
    }
}
