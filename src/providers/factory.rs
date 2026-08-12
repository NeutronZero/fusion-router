//! Config-driven provider factory.
//!
//! Replaces the hardcoded `match provider_type` blocks with a single entry
//! point that creates the right `ChatProvider` from `ProviderConfig`.

use std::sync::Arc;
use std::time::Duration;

use crate::config::{CapabilityDescriptor, ProviderConfig};
use super::circuit_breaker::CircuitBreaker;
use super::circuit_breaking_provider::CircuitBreakingProvider;
use super::generic_openai_model::GenericOpenAIModel;
use super::ollama::OllamaProvider;
use super::openrouter::OpenRouterProvider;
use super::provider_with_headers::ProviderWithHeaders;
use super::router::ProviderTarget;
use super::zen::ZenProvider;
use super::ChatProvider;

/// Default HTTP timeout for generic providers (120s).
const GENERIC_TIMEOUT: Duration = Duration::from_secs(120);

/// Resolves the API key for a provider, checking `api_key` first (direct or
/// `{env:VAR}` syntax), then `api_key_env`, then falling back to a placeholder
/// in debug/`--unsafe-dev` mode.
pub fn resolve_api_key(
    cfg: &ProviderConfig,
    provider_name: &str,
    unsafe_dev: bool,
) -> anyhow::Result<String> {
    // 1. Direct api_key field (supports `{env:VAR}` syntax)
    if let Some(direct) = &cfg.api_key {
        if let Some(var) = direct.strip_prefix("{env:").and_then(|s| s.strip_suffix('}')) {
            if let Ok(val) = std::env::var(var) {
                if !val.trim().is_empty() {
                    return Ok(val);
                }
            }
        } else if !direct.trim().is_empty() {
            return Ok(direct.clone());
        }
    }

    // 2. api_key_env
    if let Some(var) = &cfg.api_key_env {
        if let Ok(val) = std::env::var(var) {
            if !val.trim().is_empty() {
                return Ok(val);
            }
        }
    }

    // 3. Placeholder in debug/unsafe-dev
    if cfg!(debug_assertions) || unsafe_dev {
        tracing::warn!(
            provider = %provider_name,
            "no API key configured; using placeholder (debug/--unsafe-dev only)"
        );
        return Ok(format!("test-key-{}", provider_name));
    }

    anyhow::bail!(
        "provider '{}' has no API key configured; set `api_key`, `api_key_env`, or run with --unsafe-dev",
        provider_name
    )
}

/// Creates a `ProviderTarget` from a `ProviderConfig`.
///
/// Built-in types (`openrouter`, `zen`, `ollama`) use their dedicated
/// implementations. Everything else (`openai-compatible`, `deepseek`, `groq`,
/// `cerebras`, etc.) uses the generic OpenAI-compatible model.
pub fn create_provider_target(
    name: &str,
    cfg: &ProviderConfig,
    api_key: String,
) -> ProviderTarget {
    let circuit_breaker = CircuitBreaker::new(cfg.failure_threshold, 3, cfg.cooldown_secs);
    let transport = cfg.effective_transport().to_string();
    let base_url = cfg.base_url.clone();
    let provider_name = name.to_string();
    let custom_headers = cfg.headers.clone();
    let models_cfg = cfg.models.clone();

    ProviderTarget::new(
        name.to_string(),
        circuit_breaker,
        Box::new(move || -> Arc<dyn ChatProvider + Send + Sync> {
            let provider: Arc<dyn ChatProvider + Send + Sync> = match transport.as_str() {
                "openrouter" => {
                    Arc::new(OpenRouterProvider::with_base_url(
                        api_key.clone(),
                        base_url.clone(),
                    ))
                }
                "zen" | "opencode-zen" => {
                    Arc::new(ZenProvider::with_base_url(
                        api_key.clone(),
                        base_url.clone(),
                    ))
                }
                "ollama" => {
                    Arc::new(OllamaProvider::new())
                }
                _ => {
                    // Generic OpenAI-compatible provider.
                    let url = base_url.clone().unwrap_or_else(|| {
                        match transport.as_str() {
                            "deepseek" => "https://api.deepseek.com/v1".to_string(),
                            "groq" => "https://api.groq.com/openai/v1".to_string(),
                            "cerebras" => "https://api.cerebras.ai/v1".to_string(),
                            "fireworks" => "https://api.fireworks.ai/inference/v1".to_string(),
                            "together" => "https://api.together.xyz/v1".to_string(),
                            "xai" => "https://api.x.ai/v1".to_string(),
                            "nvidia" => "https://integrate.api.nvidia.com/v1".to_string(),
                            "openai" => "https://api.openai.com/v1".to_string(),
                            "anthropic" => "https://api.anthropic.com/v1".to_string(),
                            _ => "http://localhost:8080/v1".to_string(),
                        }
                    });

                    let (model_id, model_cfg) = models_cfg
                        .iter()
                        .next()
                        .map(|(id, mc)| (id.clone(), mc.clone()))
                        .unwrap_or_else(|| {
                            (format!("{}-model", provider_name), CapabilityDescriptor::default())
                        });

                    let model = GenericOpenAIModel::new(
                        model_id,
                        provider_name.clone(),
                        url,
                        &model_cfg,
                        format!("{}/", provider_name),
                    );
                    let transport = super::HttpTransport::new(GENERIC_TIMEOUT)
                        .unwrap_or_default();
                    Arc::new(super::Provider::new(
                        Box::new(model),
                        Box::new(transport),
                        api_key.clone(),
                    ))
                }
            };

            // Wrap with custom headers if any
            if custom_headers.is_empty() {
                provider
            } else {
                Arc::new(ProviderWithHeaders::new(provider, custom_headers.clone()))
            }
        }),
    )
}

/// Creates a `CircuitBreakingProvider`-wrapped target with circuit breaker
/// protection. Useful for production deployments.
pub fn create_protected_target(
    name: &str,
    cfg: &ProviderConfig,
    api_key: String,
) -> ProviderTarget {
    let circuit_breaker = CircuitBreaker::new(cfg.failure_threshold, 3, cfg.cooldown_secs);
    let transport = cfg.effective_transport().to_string();
    let base_url = cfg.base_url.clone();
    let provider_name = name.to_string();
    let custom_headers = cfg.headers.clone();
    let models_cfg = cfg.models.clone();
    let failure_threshold = cfg.failure_threshold;
    let cooldown = cfg.cooldown_secs;

    ProviderTarget::new(
        name.to_string(),
        circuit_breaker,
        Box::new(move || -> Arc<dyn ChatProvider + Send + Sync> {
            let inner: Arc<dyn ChatProvider + Send + Sync> = match transport.as_str() {
                "openrouter" => {
                    Arc::new(OpenRouterProvider::with_base_url(
                        api_key.clone(),
                        base_url.clone(),
                    ))
                }
                "zen" | "opencode-zen" => {
                    Arc::new(ZenProvider::with_base_url(
                        api_key.clone(),
                        base_url.clone(),
                    ))
                }
                "ollama" => {
                    Arc::new(OllamaProvider::new())
                }
                _ => {
                    let url = base_url.clone().unwrap_or_else(|| {
                        match transport.as_str() {
                            "deepseek" => "https://api.deepseek.com/v1".to_string(),
                            "groq" => "https://api.groq.com/openai/v1".to_string(),
                            "cerebras" => "https://api.cerebras.ai/v1".to_string(),
                            "fireworks" => "https://api.fireworks.ai/inference/v1".to_string(),
                            "together" => "https://api.together.xyz/v1".to_string(),
                            "xai" => "https://api.x.ai/v1".to_string(),
                            "nvidia" => "https://integrate.api.nvidia.com/v1".to_string(),
                            "openai" => "https://api.openai.com/v1".to_string(),
                            "anthropic" => "https://api.anthropic.com/v1".to_string(),
                            _ => "http://localhost:8080/v1".to_string(),
                        }
                    });

                    let (model_id, model_cfg) = models_cfg
                        .iter()
                        .next()
                        .map(|(id, mc)| (id.clone(), mc.clone()))
                        .unwrap_or_else(|| {
                            (format!("{}-model", provider_name), CapabilityDescriptor::default())
                        });

                    let model = GenericOpenAIModel::new(
                        model_id,
                        provider_name.clone(),
                        url,
                        &model_cfg,
                        format!("{}/", provider_name),
                    );
                    let transport = super::HttpTransport::new(GENERIC_TIMEOUT)
                        .unwrap_or_default();
                    Arc::new(super::Provider::new(
                        Box::new(model),
                        Box::new(transport),
                        api_key.clone(),
                    ))
                }
            };

            let with_headers = if custom_headers.is_empty() {
                inner
            } else {
                Arc::new(ProviderWithHeaders::new(inner, custom_headers.clone()))
            };

            Arc::new(CircuitBreakingProvider::new(
                with_headers,
                failure_threshold,
                2,
                cooldown,
                provider_name.clone(),
            ))
        }),
    )
}

/// Builds a provider target suitable for the ConfigSubscriber hot-reload path.
/// Unlike `create_provider_target`, this uses the config's base_url directly
/// (it's called from the prepare/commit lifecycle where the old config is
/// available).
pub fn create_reload_target(
    name: &str,
    cfg: &ProviderConfig,
    api_key: String,
) -> ProviderTarget {
    create_provider_target(name, cfg, api_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_resolve_api_key_direct() {
        let cfg = ProviderConfig {
            api_key: Some("sk-direct".into()),
            ..Default::default()
        };
        let key = resolve_api_key(&cfg, "test", false).unwrap();
        assert_eq!(key, "sk-direct");
    }

    #[test]
    fn test_resolve_api_key_env_syntax() {
        std::env::set_var("TEST_RESOLVE_KEY_123", "sk-from-env");
        let cfg = ProviderConfig {
            api_key: Some("{env:TEST_RESOLVE_KEY_123}".into()),
            ..Default::default()
        };
        let key = resolve_api_key(&cfg, "test", false).unwrap();
        assert_eq!(key, "sk-from-env");
        std::env::remove_var("TEST_RESOLVE_KEY_123");
    }

    #[test]
    fn test_resolve_api_key_env_field() {
        std::env::set_var("TEST_RESOLVE_KEY_456", "sk-from-env-field");
        let cfg = ProviderConfig {
            api_key_env: Some("TEST_RESOLVE_KEY_456".into()),
            ..Default::default()
        };
        let key = resolve_api_key(&cfg, "test", false).unwrap();
        assert_eq!(key, "sk-from-env-field");
        std::env::remove_var("TEST_RESOLVE_KEY_456");
    }

    #[test]
    fn test_resolve_api_key_placeholder_in_dev() {
        let cfg = ProviderConfig::default();
        let key = resolve_api_key(&cfg, "myprovider", true).unwrap();
        assert_eq!(key, "test-key-myprovider");
    }

    #[test]
    fn test_resolve_api_key_direct_takes_precedence() {
        std::env::set_var("TEST_RESOLVE_KEY_789", "sk-from-env");
        let cfg = ProviderConfig {
            api_key: Some("sk-direct".into()),
            api_key_env: Some("TEST_RESOLVE_KEY_789".into()),
            ..Default::default()
        };
        let key = resolve_api_key(&cfg, "test", false).unwrap();
        assert_eq!(key, "sk-direct");
        std::env::remove_var("TEST_RESOLVE_KEY_789");
    }

    #[test]
    fn test_create_provider_target_generic() {
        let cfg = ProviderConfig {
            transport: "deepseek".into(),
            base_url: Some("https://api.deepseek.com/v1".into()),
            ..Default::default()
        };
        let target = create_provider_target("deepseek", &cfg, "test-key".into());
        assert_eq!(target.name, "deepseek");
    }

    #[test]
    fn test_create_provider_target_with_custom_headers() {
        let mut headers = HashMap::new();
        headers.insert("X-Custom".into(), "value".into());
        let cfg = ProviderConfig {
            transport: "openai".into(),
            headers,
            ..Default::default()
        };
        let target = create_provider_target("custom", &cfg, "test-key".into());
        assert_eq!(target.name, "custom");
    }

    #[test]
    fn test_create_provider_target_openrouter() {
        let cfg = ProviderConfig {
            transport: "openrouter".into(),
            base_url: Some("https://openrouter.ai/api/v1".into()),
            ..Default::default()
        };
        let target = create_provider_target("openrouter", &cfg, "test-key".into());
        assert_eq!(target.name, "openrouter");
    }
}
