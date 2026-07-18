use std::collections::HashMap;
use serde::Deserialize;

use crate::types::{Policy, PolicyAction, PolicyCondition, Quota, ProviderLimit};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
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

fn default_host() -> String { "0.0.0.0".to_string() }
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

fn default_cors_origins() -> Vec<String> { vec!["*".into()] }
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
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub api_keys: Vec<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self { enabled: false, api_keys: vec![] }
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

fn default_rate_limiting_enabled() -> bool { false }
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
    #[serde(default)]
    pub provider_limits: HashMap<String, ProviderLimitConfig>,
}

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
    vec!["ls".into(), "echo".into(), "cat".into(), "cmd".into()]
}

fn default_shell_timeout_secs() -> u64 { 10 }

fn default_allowed_read_directories() -> Vec<String> {
    vec![".".into()]
}

fn default_enable_http_tool() -> bool { true }

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

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors: Vec<String> = Vec::new();

        if self.server.port == 0 {
            errors.push("server.port must be greater than 0".into());
        }

        if self.server.shutdown_timeout_secs == 0 {
            errors.push("server.shutdown_timeout_secs must be greater than 0".into());
        }

        if self.resources.max_daily_cost < 0.0 {
            errors.push("resources.max_daily_cost must be non-negative".into());
        }

        if self.resources.max_concurrent == 0 {
            errors.push("resources.max_concurrent must be greater than 0".into());
        }

        if self.auth.enabled && self.auth.api_keys.is_empty() {
            errors.push("auth.enabled is true but no api_keys configured".into());
        }

        if self.rate_limiting.enabled {
            if self.rate_limiting.requests_per_minute == 0 {
                errors.push("rate_limiting.requests_per_minute must be greater than 0".into());
            }
            if self.rate_limiting.burst_size == 0 {
                errors.push("rate_limiting.burst_size must be greater than 0".into());
            }
            if self.rate_limiting.cleanup_interval_secs == 0 {
                errors.push("rate_limiting.cleanup_interval_secs must be greater than 0".into());
            }
        }

        match self.logging.format.as_str() {
            "text" | "json" => {}
            other => errors.push(format!("logging.format must be 'text' or 'json', got '{}'", other)),
        }

        if self.logging.level.is_empty() {
            errors.push("logging.level must not be empty".into());
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
