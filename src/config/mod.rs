pub mod error;
pub mod manager;

use std::collections::HashMap;
use serde::Deserialize;

use crate::config::error::{ConfigValidationError, ValidationSeverity};
use crate::feature_gate::FeatureConfig;
use crate::types::{Policy, PolicyAction, PolicyCondition, Quota, ProviderLimit};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub unsafe_dev: bool,
    pub server: ServerConfig,
    pub resources: ResourceConfig,
    #[serde(default)]
    pub policies: Vec<PolicyConfig>,
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub strategies: StrategyConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub rate_limiting: RateLimitingConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub model_catalog: crate::types::ModelCatalog,
    #[serde(default)]
    pub connectors: HashMap<String, ConnectorConfig>,
    #[serde(default)]
    pub features: HashMap<String, FeatureConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout_secs: u64,
    #[serde(default)]
    pub cors: CorsConfig,
}

fn default_host() -> String { "127.0.0.1".to_string() }
fn default_port() -> u16 { 8080 }
fn default_shutdown_timeout() -> u64 { 30 }

#[derive(Debug, Clone, Deserialize)]
pub struct CorsConfig {
    #[serde(default = "default_cors_origins")]
    pub allowed_origins: Vec<String>,
    #[serde(default = "default_cors_methods")]
    pub allowed_methods: Vec<String>,
    #[serde(default = "default_cors_headers")]
    pub allowed_headers: Vec<String>,
}

fn default_cors_origins() -> Vec<String> { vec![] }
fn default_cors_methods() -> Vec<String> { vec!["GET".into(), "POST".into(), "PUT".into(), "DELETE".into(), "OPTIONS".into()] }
fn default_cors_headers() -> Vec<String> { vec!["content-type".into(), "authorization".into(), "x-api-key".into(), "x-request-id".into()] }

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: default_cors_origins(),
            allowed_methods: default_cors_methods(),
            allowed_headers: default_cors_headers(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_auth_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub api_keys: Vec<String>,
}

fn default_auth_enabled() -> bool { true }

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: default_auth_enabled(),
            api_keys: Vec::new(),
        }
    }
}


#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitingConfig {
    #[serde(default = "default_rate_limiting_enabled")]
    pub enabled: bool,
    #[serde(default = "default_rpm")]
    pub requests_per_minute: u64,
    #[serde(default = "default_burst")]
    pub burst_size: u32,
    #[serde(default = "default_cleanup_interval")]
    pub cleanup_interval_secs: u64,
}

fn default_rate_limiting_enabled() -> bool { true }
fn default_rpm() -> u64 { 60 }
fn default_burst() -> u32 { 10 }
fn default_cleanup_interval() -> u64 { 300 }

impl Default for RateLimitingConfig {
    fn default() -> Self {
        Self {
            enabled: default_rate_limiting_enabled(),
            requests_per_minute: default_rpm(),
            burst_size: default_burst(),
            cleanup_interval_secs: default_cleanup_interval(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_format")]
    pub format: String,
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default)]
    pub directory: Option<String>,
}

fn default_log_format() -> String { "text".into() }
fn default_log_level() -> String { "info".into() }

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            format: default_log_format(),
            level: default_log_level(),
            directory: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResourceConfig {
    pub max_daily_cost: f64,
    pub max_daily_tokens: u64,
    #[serde(default = "default_concurrent")]
    pub max_concurrent: u32,
    #[serde(default = "default_max_concurrent_nodes")]
    pub max_concurrent_nodes: u32,
    #[serde(default)]
    pub provider_limits: HashMap<String, ProviderLimitConfig>,
}

fn default_max_concurrent_nodes() -> u32 { 16 }

fn default_concurrent() -> u32 { 5 }

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderLimitConfig {
    pub max_daily_cost: f64,
    pub max_rpm: u32,
    pub max_tpm: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyConfig {
    pub name: String,
    #[serde(default)]
    pub priority: u32,
    #[serde(default)]
    pub conditions: Vec<PolicyConditionConfig>,
    #[serde(default)]
    pub actions: Vec<PolicyActionConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyConditionConfig {
    pub field: String,
    pub operator: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyActionConfig {
    pub action_type: String,
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
    #[serde(default = "default_cooldown_secs")]
    pub cooldown_secs: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ConnectorConfig {
    pub connector_type: String,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StrategyConfig {
    #[serde(default = "default_consensus_count")]
    pub consensus_count: u32,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self { consensus_count: default_consensus_count() }
    }
}

fn default_consensus_count() -> u32 { 3 }

fn default_failure_threshold() -> u32 { 5 }
fn default_cooldown_secs() -> u64 { 30 }

#[derive(Debug, Clone, Deserialize)]
pub struct ToolsConfig {
    #[serde(default = "default_allowed_shell_commands")]
    pub allowed_shell_commands: Vec<String>,
    #[serde(default = "default_shell_timeout_secs")]
    pub shell_timeout_secs: u64,
    #[serde(default = "default_allowed_read_directories")]
    pub allowed_read_directories: Vec<String>,
    #[serde(default = "default_enable_http_tool")]
    pub enable_http_tool: bool,
}

fn default_allowed_shell_commands() -> Vec<String> {
    vec![]
}

fn default_shell_timeout_secs() -> u64 { 10 }

fn default_allowed_read_directories() -> Vec<String> {
    vec![".".into()]
}

fn default_enable_http_tool() -> bool { false }

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            allowed_shell_commands: default_allowed_shell_commands(),
            shell_timeout_secs: default_shell_timeout_secs(),
            allowed_read_directories: default_allowed_read_directories(),
            enable_http_tool: default_enable_http_tool(),
        }
    }
}

impl AppConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: AppConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    pub fn to_quota(&self) -> Quota {
        Quota {
            max_daily_cost: self.resources.max_daily_cost,
            max_daily_tokens: self.resources.max_daily_tokens,
            max_concurrent: self.resources.max_concurrent,
            provider_limits: self.resources.provider_limits.iter().map(|(k, v)| {
                (k.clone(), ProviderLimit {
                    max_daily_cost: v.max_daily_cost,
                    max_rpm: v.max_rpm,
                    max_tpm: v.max_tpm,
                })
            }).collect(),
        }
    }

    pub fn validate(&self) -> Result<(), Vec<ConfigValidationError>> {
        // Release builds enforce fail-closed deployment posture (ADR-035);
        // debug builds skip these so local development stays frictionless.
        self.validate_with_profile(!cfg!(debug_assertions))
    }

    /// Same as `validate()`, with the profile explicit so tests can exercise
    /// the release-mode fail-closed checks regardless of the build profile.
    pub fn validate_with_profile(&self, release: bool) -> Result<(), Vec<ConfigValidationError>> {
        let mut errors: Vec<ConfigValidationError> = Vec::new();

        if self.server.port == 0 {
            errors.push(ConfigValidationError {
                field: "server.port".into(),
                message: "port must be > 0".into(),
                value: Some(self.server.port.to_string()),
                severity: ValidationSeverity::Error,
            });
        }

        if self.server.shutdown_timeout_secs == 0 {
            errors.push(ConfigValidationError {
                field: "server.shutdown_timeout_secs".into(),
                message: "shutdown_timeout_secs must be > 0".into(),
                value: Some(self.server.shutdown_timeout_secs.to_string()),
                severity: ValidationSeverity::Error,
            });
        }

        if self.resources.max_daily_cost < 0.0 {
            errors.push(ConfigValidationError {
                field: "resources.max_daily_cost".into(),
                message: "max_daily_cost must be non-negative".into(),
                value: Some(self.resources.max_daily_cost.to_string()),
                severity: ValidationSeverity::Error,
            });
        }

        if self.resources.max_concurrent == 0 {
            errors.push(ConfigValidationError {
                field: "resources.max_concurrent".into(),
                message: "max_concurrent must be > 0".into(),
                value: Some(self.resources.max_concurrent.to_string()),
                severity: ValidationSeverity::Error,
            });
        }

        if self.resources.max_concurrent_nodes == 0 {
            errors.push(ConfigValidationError {
                field: "resources.max_concurrent_nodes".into(),
                message: "max_concurrent_nodes must be > 0".into(),
                value: Some(self.resources.max_concurrent_nodes.to_string()),
                severity: ValidationSeverity::Error,
            });
        }

        if self.auth.enabled && self.auth.api_keys.is_empty() && !self.unsafe_dev {
            errors.push(ConfigValidationError {
                field: "auth.api_keys".into(),
                message: "auth is enabled but no api_keys configured".into(),
                value: None,
                severity: ValidationSeverity::Error,
            });
        }

        if self.rate_limiting.enabled {
            if self.rate_limiting.requests_per_minute == 0 {
                errors.push(ConfigValidationError {
                    field: "rate_limiting.requests_per_minute".into(),
                    message: "requests_per_minute must be > 0".into(),
                    value: Some(self.rate_limiting.requests_per_minute.to_string()),
                    severity: ValidationSeverity::Error,
                });
            }
            if self.rate_limiting.burst_size == 0 {
                errors.push(ConfigValidationError {
                    field: "rate_limiting.burst_size".into(),
                    message: "burst_size must be > 0".into(),
                    value: Some(self.rate_limiting.burst_size.to_string()),
                    severity: ValidationSeverity::Error,
                });
            }
            if self.rate_limiting.cleanup_interval_secs == 0 {
                errors.push(ConfigValidationError {
                    field: "rate_limiting.cleanup_interval_secs".into(),
                    message: "cleanup_interval_secs must be > 0".into(),
                    value: Some(self.rate_limiting.cleanup_interval_secs.to_string()),
                    severity: ValidationSeverity::Error,
                });
            }
        }

        // ADR-035: release builds fail closed on insecure combinations.
        // `--unsafe-dev` (AppConfig::unsafe_dev) is the only escape hatch.
        if release && !self.unsafe_dev {
            if !self.auth.enabled {
                errors.push(ConfigValidationError {
                    field: "auth.enabled".into(),
                    message: "authentication is disabled; start with --unsafe-dev to run without auth".into(),
                    value: None,
                    severity: ValidationSeverity::Error,
                });
            }
            if !self.rate_limiting.enabled {
                errors.push(ConfigValidationError {
                    field: "rate_limiting.enabled".into(),
                    message: "rate limiting is disabled; start with --unsafe-dev to run without rate limiting".into(),
                    value: None,
                    severity: ValidationSeverity::Error,
                });
            }
            if self.server.cors.allowed_origins.iter().any(|o| o == "*") {
                errors.push(ConfigValidationError {
                    field: "server.cors.allowed_origins".into(),
                    message: "wildcard CORS origin '*' is forbidden; start with --unsafe-dev to allow it".into(),
                    value: None,
                    severity: ValidationSeverity::Error,
                });
            }
            if !self.tools.allowed_shell_commands.is_empty() {
                errors.push(ConfigValidationError {
                    field: "tools.allowed_shell_commands".into(),
                    message: "shell commands are disabled by default; start with --unsafe-dev to allow them".into(),
                    value: None,
                    severity: ValidationSeverity::Error,
                });
            }
            if self.tools.enable_http_tool {
                errors.push(ConfigValidationError {
                    field: "tools.enable_http_tool".into(),
                    message: "the HTTP tool is disabled by default; start with --unsafe-dev to enable it".into(),
                    value: None,
                    severity: ValidationSeverity::Error,
                });
            }
        }

        match self.logging.format.as_str() {
            "text" | "json" => {}
            other => errors.push(ConfigValidationError {
                field: "logging.format".into(),
                message: "format must be 'text' or 'json'".into(),
                value: Some(other.into()),
                severity: ValidationSeverity::Error,
            }),
        }

        if self.logging.level.is_empty() {
            errors.push(ConfigValidationError {
                field: "logging.level".into(),
                message: "level must not be empty".into(),
                value: None,
                severity: ValidationSeverity::Error,
            });
        }

        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    pub fn to_policies(&self) -> Vec<Policy> {
        self.policies.iter().map(|p| Policy {
            name: p.name.clone(),
            priority: p.priority,
            conditions: p.conditions.iter().map(|c| PolicyCondition {
                field: c.field.clone(),
                operator: c.operator.clone(),
                value: c.value.clone(),
            }).collect(),
            actions: p.actions.iter().map(|a| PolicyAction {
                action_type: a.action_type.clone(),
                params: a.params.clone(),
            }).collect(),
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert_eq!(quota.max_daily_cost, 42.5);
        assert_eq!(quota.max_daily_tokens, 999);
        assert_eq!(quota.max_concurrent, 7);
        let limit = quota.provider_limits.get("openai").unwrap();
        assert_eq!(limit.max_rpm, 60);
        assert_eq!(limit.max_tpm, 100_000);
        assert_eq!(limit.max_daily_cost, 10.0);
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
