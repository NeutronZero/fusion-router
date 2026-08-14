pub mod defaults;
pub mod error;
pub mod manager;
pub mod types;
pub mod validation;

pub use types::*;


#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::config::defaults::*;
    use crate::config::error::*;

    fn base_config() -> AppConfig {
        AppConfig {
            unsafe_dev: false,
            server: ServerConfig {
                host: "0.0.0.0".into(),
                port: 8080,
                shutdown_timeout_secs: 30,
                cors: CorsConfig::default(),
            },
            resources: ResourceConfig {
                max_daily_cost: 100.0,
                max_daily_tokens: 1_000_000,
                max_concurrent: 5,
                max_concurrent_nodes: 16,
                provider_limits: HashMap::new(),
            },
            policies: Vec::new(),
            providers: HashMap::new(),
            strategies: StrategyConfig::default(),
            tools: ToolsConfig::default(),
            auth: AuthConfig { enabled: true, api_keys: vec!["sk-test".into()] },
            rate_limiting: RateLimitingConfig::default(),
            logging: LoggingConfig::default(),
            model_catalog: crate::types::ModelCatalog::default(),
            connectors: HashMap::new(),
            features: HashMap::new(),
        }
    }

    #[test]
    fn test_defaults_are_fail_closed() {
        assert_eq!(default_host(), "127.0.0.1");
        assert_eq!(default_cors_origins(), Vec::<String>::new());
        assert!(default_rate_limiting_enabled());
        assert_eq!(default_allowed_shell_commands(), Vec::<String>::new());
        assert!(!default_enable_http_tool());
        assert!(AuthConfig::default().enabled, "auth must default to enabled");
        let mut config = base_config();
        config.auth = AuthConfig::default();
        assert!(config.auth.enabled && config.auth.api_keys.is_empty());
    }

    #[test]
    fn test_unsafe_dev_defaults_false_via_deserialization() {
        let yaml = r#"
server:
  port: 8080
resources:
  max_daily_cost: 10.0
  max_daily_tokens: 1000000
"#;
        let config: AppConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!config.unsafe_dev, "unsafe_dev must default to false");
        assert_eq!(config.server.host, "127.0.0.1");
        assert!(config.auth.enabled, "auth must default to enabled when deserialized");
        assert!(config.rate_limiting.enabled, "rate limiting must default to enabled");
        assert!(config.server.cors.allowed_origins.is_empty());
        assert!(config.tools.allowed_shell_commands.is_empty());
        assert!(!config.tools.enable_http_tool);
        assert!(!config.tools.allow_auto_exec, "tool auto-execution must default to false");
        assert!(
            !config.tools.allow_unrestricted_args,
            "unrestricted shell args must default to false"
        );
        // New provider config defaults
        let p = ProviderConfig::default();
        assert_eq!(p.transport, "openai-chat");
        assert!(p.api_key.is_none());
        assert!(p.headers.is_empty());
        assert!(p.models.is_empty());
        assert!(p.blacklist.is_empty());
        assert!(p.whitelist.is_empty());
        assert!(p.options.is_empty());
    }

    #[test]
    fn test_tools_config_defaults_are_fail_closed() {
        let tools = ToolsConfig::default();
        assert!(!tools.allow_auto_exec);
        assert!(!tools.allow_unrestricted_args);
        assert!(!default_allow_auto_exec());
        assert!(!default_allow_unrestricted_args());
    }

    #[test]
    fn test_release_validate_rejects_auth_disabled_without_unsafe_dev() {
        let mut config = base_config();
        config.auth.enabled = false;
        let errors = config.validate_with_profile(true).unwrap_err();
        assert!(errors.iter().any(|e| e.field == "auth.enabled"));
    }

    #[test]
    fn test_release_validate_rejects_rate_limit_disabled_without_unsafe_dev() {
        let mut config = base_config();
        config.rate_limiting.enabled = false;
        let errors = config.validate_with_profile(true).unwrap_err();
        assert!(errors.iter().any(|e| e.field == "rate_limiting.enabled"));
    }

    #[test]
    fn test_release_validate_rejects_wildcard_cors_without_unsafe_dev() {
        let mut config = base_config();
        config.server.cors.allowed_origins = vec!["*".into()];
        let errors = config.validate_with_profile(true).unwrap_err();
        assert!(errors.iter().any(|e| e.field == "server.cors.allowed_origins"));
    }

    #[test]
    fn test_release_validate_rejects_permissive_tools_without_unsafe_dev() {
        let mut config = base_config();
        config.tools.allowed_shell_commands = vec!["cat".into()];
        config.tools.enable_http_tool = true;
        let errors = config.validate_with_profile(true).unwrap_err();
        assert!(errors.iter().any(|e| e.field == "tools.allowed_shell_commands"));
        assert!(errors.iter().any(|e| e.field == "tools.enable_http_tool"));
    }

    #[test]
    fn test_release_validate_rejects_auth_enabled_without_keys() {
        let mut config = base_config();
        config.auth.api_keys = vec![];
        let errors = config.validate_with_profile(true).unwrap_err();
        assert!(errors.iter().any(|e| e.field == "auth.api_keys"));
    }

    #[test]
    fn test_unsafe_dev_allows_insecure_configuration() {
        let mut config = base_config();
        config.unsafe_dev = true;
        config.auth.enabled = false;
        config.auth.api_keys = vec![];
        config.rate_limiting.enabled = false;
        config.server.cors.allowed_origins = vec!["*".into()];
        config.tools.allowed_shell_commands = vec!["cat".into()];
        config.tools.enable_http_tool = true;
        assert!(
            config.validate_with_profile(true).is_ok(),
            "unsafe_dev must be the escape hatch for every insecure combination"
        );
    }

    #[test]
    fn test_debug_profile_skips_release_checks() {
        let mut config = base_config();
        config.auth.enabled = false;
        config.rate_limiting.enabled = false;
        config.server.cors.allowed_origins = vec!["*".into()];
        config.tools.enable_http_tool = true;
        assert!(config.validate_with_profile(false).is_ok());
    }

    #[test]
    fn test_validate_accepts_minimal_config() {
        assert!(base_config().validate().is_ok());
    }

    #[test]
    fn test_validate_rejects_zero_port() {
        let mut config = base_config();
        config.server.port = 0;
        let errors = config.validate().unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field, "server.port");
        assert!(matches!(errors[0].severity, ValidationSeverity::Error));
    }

    #[test]
    fn test_validate_rejects_zero_shutdown_timeout() {
        let mut config = base_config();
        config.server.shutdown_timeout_secs = 0;
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.field == "server.shutdown_timeout_secs"));
    }

    #[test]
    fn test_validate_rejects_negative_cost() {
        let mut config = base_config();
        config.resources.max_daily_cost = -1.0;
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.field == "resources.max_daily_cost"));
    }

    #[test]
    fn test_validate_rejects_zero_concurrency() {
        let mut config = base_config();
        config.resources.max_concurrent = 0;
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.field == "resources.max_concurrent"));
    }

    #[test]
    fn test_validate_rejects_auth_without_keys() {
        let mut config = base_config();
        config.auth.enabled = true;
        config.auth.api_keys = vec![];
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.field == "auth.api_keys"));
    }

    #[test]
    fn test_validate_accepts_auth_with_keys() {
        let mut config = base_config();
        config.auth.enabled = true;
        config.auth.api_keys = vec!["sk-test".into()];
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_rejects_zero_rate_limit_settings() {
        let mut config = base_config();
        config.rate_limiting.enabled = true;
        config.rate_limiting.requests_per_minute = 0;
        config.rate_limiting.burst_size = 0;
        config.rate_limiting.cleanup_interval_secs = 0;
        let errors = config.validate().unwrap_err();
        assert_eq!(errors.len(), 3);
    }

    #[test]
    fn test_validate_rejects_invalid_log_format() {
        let mut config = base_config();
        config.logging.format = "xml".into();
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.field == "logging.format"));
    }

    #[test]
    fn test_validate_rejects_empty_log_level() {
        let mut config = base_config();
        config.logging.level = String::new();
        let errors = config.validate().unwrap_err();
        assert!(errors.iter().any(|e| e.field == "logging.level"));
    }

    #[test]
    fn test_validate_reports_multiple_errors() {
        let mut config = base_config();
        config.server.port = 0;
        config.resources.max_concurrent = 0;
        config.auth.api_keys = vec![];
        let errors = config.validate().unwrap_err();
        assert_eq!(errors.len(), 3);
    }

    #[test]
    fn test_to_quota_maps_resource_fields() {
        let mut config = base_config();
        config.resources.max_daily_cost = 42.5;
        config.resources.max_daily_tokens = 999;
        config.resources.max_concurrent = 7;
        config.resources.provider_limits.insert(
            "openai".into(),
            ProviderLimitConfig {
                max_daily_cost: 10.0,
                max_rpm: 60,
                max_tpm: 100_000,
            },
        );

        let quota = config.to_quota();

        assert_eq!(quota.max_daily_cost, crate::types::NanoUSD::from_nanos(42_500_000_000));
        assert_eq!(quota.max_daily_tokens, 999);
        assert_eq!(quota.max_concurrent, 7);
        let limit = quota.provider_limits.get("openai").unwrap();
        assert_eq!(limit.max_rpm, 60);
        assert_eq!(limit.max_tpm, 100_000);
        assert_eq!(limit.max_daily_cost, crate::types::NanoUSD::from_nanos(10_000_000_000));
    }

    #[test]
    fn test_to_quota_empty_provider_limits() {
        let quota = base_config().to_quota();
        assert!(quota.provider_limits.is_empty());
    }

    #[test]
    fn test_to_policies_converts_all_fields() {
        let mut config = base_config();
        config.policies = vec![PolicyConfig {
            name: "cost-cap".into(),
            priority: 7,
            conditions: vec![PolicyConditionConfig {
                field: "cost".into(),
                operator: "gt".into(),
                value: serde_json::json!(0.05),
            }],
            actions: vec![PolicyActionConfig {
                action_type: "deny".into(),
                params: HashMap::from([("reason".into(), serde_json::json!("budget"))]),
            }],
        }];

        let policies = config.to_policies();

        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].name, "cost-cap");
        assert_eq!(policies[0].priority, 7);
        assert_eq!(policies[0].conditions[0].field, "cost");
        assert_eq!(policies[0].conditions[0].operator, "gt");
        assert_eq!(policies[0].conditions[0].value, serde_json::json!(0.05));
        assert_eq!(policies[0].actions[0].action_type, "deny");
        assert_eq!(
            policies[0].actions[0].params.get("reason"),
            Some(&serde_json::json!("budget"))
        );
    }

    #[test]
    fn test_to_policies_empty() {
        assert!(base_config().to_policies().is_empty());
    }
}
