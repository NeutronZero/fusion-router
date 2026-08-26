use crate::types::NanoUSD;
use serde::Deserialize;
use std::collections::HashMap;

use crate::config::defaults::*;
use crate::feature_gate::FeatureConfig;

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
    /// Server-wide per-request ceiling. Streaming responses that exceed it
    /// are cut; non-streaming requests fail with 504. Default 300s.
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
    #[serde(default)]
    pub cors: CorsConfig,
}

fn default_request_timeout_secs() -> u64 {
    300
}

#[derive(Debug, Clone, Deserialize)]
pub struct CorsConfig {
    #[serde(default = "default_cors_origins")]
    pub allowed_origins: Vec<String>,
    #[serde(default = "default_cors_methods")]
    pub allowed_methods: Vec<String>,
    #[serde(default = "default_cors_headers")]
    pub allowed_headers: Vec<String>,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: default_cors_origins(),
            allowed_methods: default_cors_methods(),
            allowed_headers: default_cors_headers(),
        }
    }
}

#[derive(Clone, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_auth_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub api_keys: Vec<String>,
}

// Redacted Debug: auth keys must never reach logs/panic messages via
// `{:?}` of a config struct.
impl std::fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthConfig")
            .field("enabled", &self.enabled)
            .field(
                "api_keys",
                &format!("[REDACTED; {} key(s)]", self.api_keys.len()),
            )
            .finish()
    }
}

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
    #[serde(deserialize_with = "deserialize_usd")]
    pub max_daily_cost: NanoUSD,
    pub max_daily_tokens: u64,
    #[serde(default = "default_concurrent")]
    pub max_concurrent: u32,
    #[serde(default = "default_max_concurrent_nodes")]
    pub max_concurrent_nodes: u32,
    #[serde(default)]
    pub provider_limits: HashMap<String, ProviderLimitConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderLimitConfig {
    #[serde(deserialize_with = "deserialize_usd")]
    pub max_daily_cost: NanoUSD,
    pub max_rpm: u32,
    pub max_tpm: u64,
}

fn deserialize_usd<'de, D>(deserializer: D) -> Result<NanoUSD, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Money {
        Integer(u64),
        Decimal(f64),
        Text(String),
    }
    let value = Money::deserialize(deserializer)?;
    let text = match value {
        Money::Integer(value) => value.to_string(),
        Money::Decimal(value) => value.to_string(),
        Money::Text(value) => value,
    };
    NanoUSD::checked_from_decimal_usd(&text).map_err(serde::de::Error::custom)
}

fn deserialize_usd_opt<'de, D>(deserializer: D) -> Result<Option<NanoUSD>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Money {
        Integer(u64),
        Decimal(f64),
        Text(String),
    }
    let value = Option::<Money>::deserialize(deserializer)?;
    value
        .map(|money| {
            let text = match money {
                Money::Integer(value) => value.to_string(),
                Money::Decimal(value) => value.to_string(),
                Money::Text(value) => value,
            };
            NanoUSD::checked_from_decimal_usd(&text).map_err(serde::de::Error::custom)
        })
        .transpose()
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

#[derive(Clone, Deserialize)]
pub struct ProviderConfig {
    /// Wire protocol / transport adapter: `openai-chat`, `anthropic`, `gemini`,
    /// `ollama`, `grpc`, `websocket`, `custom`, or any OpenAI-compatible endpoint.
    #[serde(default = "default_transport")]
    pub transport: String,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    /// Direct API key or `"{env:VAR_NAME}"` syntax. Takes precedence over `api_key_env`.
    pub api_key: Option<String>,
    /// AES-256-GCM ciphertext (base64, produced by `SecretManager::encrypt`)
    /// decrypted at startup with `FUSION_MASTER_KEY`. Used only when
    /// `api_key`/`api_key_env` are unset; startup fails closed if the master
    /// key is missing or the ciphertext does not decrypt.
    #[serde(default)]
    pub api_key_encrypted: Option<String>,
    /// Custom headers sent with every request to this provider.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Per-model capability descriptors.
    #[serde(default)]
    pub models: HashMap<String, CapabilityDescriptor>,
    /// Hide these model IDs from the model picker / catalog.
    #[serde(default)]
    pub blacklist: Vec<String>,
    /// Hide every model *except* these IDs. Empty means no restriction.
    #[serde(default)]
    pub whitelist: Vec<String>,
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
    #[serde(default = "default_cooldown_secs")]
    pub cooldown_secs: u64,
    /// Provider-specific options (e.g. `region` for Bedrock, `instanceUrl` for GitLab).
    #[serde(default)]
    pub options: HashMap<String, serde_json::Value>,

    // Legacy alias: `provider_type` deserializes into `transport`.
    #[serde(default, alias = "provider_type")]
    pub(crate) _legacy_provider_type: Option<String>,
}

// Manual redacted Debug (the derived impl would print raw `api_key`,
// `api_key_encrypted`, custom header values, and provider option values).
impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("transport", &self.transport)
            .field("base_url", &self.base_url)
            .field("api_key_env", &self.api_key_env)
            .field(
                "api_key",
                &self.api_key.as_ref().map(|_| "[REDACTED]".to_string()),
            )
            .field(
                "api_key_encrypted",
                &self
                    .api_key_encrypted
                    .as_ref()
                    .map(|_| "[REDACTED]".to_string()),
            )
            // Header/option VALUES can carry secrets; only names are shown.
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("models_count", &self.models.len())
            .field("blacklist", &self.blacklist)
            .field("whitelist", &self.whitelist)
            .field("failure_threshold", &self.failure_threshold)
            .field("cooldown_secs", &self.cooldown_secs)
            .field("options_keys", &self.options.keys().collect::<Vec<_>>())
            .field("_legacy_provider_type", &self._legacy_provider_type)
            .finish()
    }
}

/// Full capability descriptor for a model.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CapabilityDescriptor {
    pub name: Option<String>,
    pub context_limit: Option<u32>,
    pub output_limit: Option<u32>,
    pub coding_score: Option<f32>,
    pub reasoning_score: Option<f32>,
    pub supports_tools: Option<bool>,
    pub supports_streaming: Option<bool>,
    pub supports_vision: Option<bool>,
    pub supports_audio: Option<bool>,
    pub supports_pdf: Option<bool>,
    pub supports_json_mode: Option<bool>,
    pub supports_thinking: Option<bool>,
    pub supports_parallel_tools: Option<bool>,
    pub supports_structured_output: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_usd_opt")]
    pub input_cost_per_1k: Option<NanoUSD>,
    #[serde(default, deserialize_with = "deserialize_usd_opt")]
    pub output_cost_per_1k: Option<NanoUSD>,
    pub latency_ms: Option<u64>,
    pub availability: Option<f32>,
    pub reliability: Option<f32>,
    pub tokenizer: Option<String>,
}

impl ProviderConfig {
    pub fn effective_transport(&self) -> &str {
        if !self.transport.is_empty() && self.transport != "openai-compatible" {
            return &self.transport;
        }
        if let Some(legacy) = &self._legacy_provider_type {
            return legacy;
        }
        &self.transport
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            transport: default_transport(),
            base_url: None,
            api_key_env: None,
            api_key: None,
            api_key_encrypted: None,
            headers: HashMap::new(),
            models: HashMap::new(),
            blacklist: Vec::new(),
            whitelist: Vec::new(),
            failure_threshold: default_failure_threshold(),
            cooldown_secs: default_cooldown_secs(),
            options: HashMap::new(),
            _legacy_provider_type: None,
        }
    }
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
        Self {
            consensus_count: default_consensus_count(),
        }
    }
}

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
    #[serde(default = "default_allow_auto_exec")]
    pub allow_auto_exec: bool,
    #[serde(default = "default_allow_unrestricted_args")]
    pub allow_unrestricted_args: bool,
    /// Shell path-argument policy (ADR-041): `stage` copies validated files
    /// into a host-controlled staging dir and rewrites argv to the staged
    /// copy, closing the validate-vs-open TOCTOU window; `direct` passes the
    /// original path (legacy behavior, warned on in release profile).
    #[serde(default = "default_shell_path_mode")]
    pub shell_path_mode: String,
    /// Upper bound for staged snapshot copies (ADR-041). Larger inputs fail
    /// closed instead of being streamed to the child.
    #[serde(default = "default_max_staged_input_bytes")]
    pub max_staged_input_bytes: usize,
}

pub fn default_shell_path_mode() -> String {
    "stage".to_string()
}

pub fn default_max_staged_input_bytes() -> usize {
    64 * 1024 * 1024
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            allowed_shell_commands: default_allowed_shell_commands(),
            shell_timeout_secs: default_shell_timeout_secs(),
            allowed_read_directories: default_allowed_read_directories(),
            enable_http_tool: default_enable_http_tool(),
            allow_auto_exec: default_allow_auto_exec(),
            allow_unrestricted_args: default_allow_unrestricted_args(),
            shell_path_mode: default_shell_path_mode(),
            max_staged_input_bytes: default_max_staged_input_bytes(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_config_debug_redacts_api_key() {
        let mut cfg = ProviderConfig::default();
        cfg.api_key = Some("sk-super-secret-value".to_string());
        cfg.api_key_encrypted = Some("ciphertext-abc".to_string());
        cfg.headers
            .insert("X-Vault-Token".to_string(), "vault-secret".to_string());

        let rendered = format!("{cfg:?}");
        assert!(!rendered.contains("sk-super-secret-value"), "{rendered}");
        assert!(!rendered.contains("ciphertext-abc"), "{rendered}");
        assert!(
            !rendered.contains("vault-secret"),
            "header values must not render"
        );
        assert!(rendered.contains("[REDACTED]"), "{rendered}");
        assert!(
            rendered.contains("X-Vault-Token"),
            "header names are not secrets and stay visible"
        );
    }

    #[test]
    fn test_auth_config_debug_redacts_keys() {
        let auth = AuthConfig {
            enabled: true,
            api_keys: vec!["key-one".into(), "key-two".into()],
        };
        let rendered = format!("{auth:?}");
        assert!(!rendered.contains("key-one"));
        assert!(!rendered.contains("key-two"));
        assert!(rendered.contains("REDACTED"));
        assert!(rendered.contains("2 key(s)"));
    }
}
