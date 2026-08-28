//! Config-driven provider factory.
//!
//! Replaces the hardcoded `match provider_type` blocks with a single entry
//! point that creates the right `ChatProvider` from `ProviderConfig`.

use std::sync::Arc;
use std::time::Duration;

/// Only these shapes of env-var names may be referenced via `{env:VAR}`
/// interpolation in `api_key`. This prevents the interpolation primitive from
/// reading arbitrary environment contents (an exfil vector if provider config
/// ever becomes attacker-influenced). Fail-closed: anything outside the
/// allowlist is rejected.
fn is_allowed_interpolation_var(var: &str) -> bool {
    let var = var.trim();
    if var.is_empty() {
        return false;
    }
    let suffix_ok = var.ends_with("_KEY")
        || var.ends_with("_TOKEN")
        || var.ends_with("_SECRET")
        || var.ends_with("_PASSWORD");
    suffix_ok
        && var
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

use super::circuit_breaker::CircuitBreaker;
use super::circuit_breaking_provider::CircuitBreakingProvider;
use super::generic_openai_model::GenericOpenAIModel;
use super::ollama::OllamaProvider;
use super::openrouter::OpenRouterProvider;
use super::provider_with_headers::ProviderWithHeaders;
use super::router::ProviderTarget;
use super::zen::ZenProvider;
use super::ChatProvider;
use crate::config::{CapabilityDescriptor, ProviderConfig};

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
        if let Some(var) = direct
            .strip_prefix("{env:")
            .and_then(|s| s.strip_suffix('}'))
        {
            if !is_allowed_interpolation_var(var) {
                anyhow::bail!(
                    "provider '{}': env interpolation of '{var}' is not allowed; only key/token/secret env var names may be interpolated",
                    provider_name
                );
            }
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

    // 3. Encrypted at rest (AES-256-GCM via SecretManager + FUSION_MASTER_KEY).
    // Fail closed: a configured ciphertext that cannot be decrypted is a
    // startup error, never an empty key.
    if let Some(encrypted) = &cfg.api_key_encrypted {
        if !encrypted.trim().is_empty() {
            let manager = crate::security::secrets::SecretManager::from_env()
                .map_err(|e| anyhow::anyhow!("provider '{provider_name}': {e}"))?;
            return manager
                .decrypt(encrypted)
                .map_err(|e| anyhow::anyhow!("provider '{provider_name}': {e}"));
        }
    }

    // 4. Placeholder in unsafe-dev
    if unsafe_dev {
        tracing::warn!(
            provider = %provider_name,
            "no API key configured; using placeholder (--unsafe-dev only)"
        );
        return Ok(format!("test-key-{}", provider_name));
    }

    anyhow::bail!(
        "provider '{}' has no API key configured; set `api_key`, `api_key_env`, `api_key_encrypted`, or run with --unsafe-dev",
        provider_name
    )
}

/// Creates a `ProviderTarget` from a `ProviderConfig`.
///
/// Built-in types (`openrouter`, `zen`, `ollama`) use their dedicated
/// implementations. Everything else (`openai-compatible`, `deepseek`, `groq`,
/// `cerebras`, etc.) uses the generic OpenAI-compatible model.
pub fn create_provider_target(name: &str, cfg: &ProviderConfig, api_key: String) -> ProviderTarget {
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
            // Every arm yields the concrete `Provider` so config-driven custom
            // headers can be injected uniformly onto outgoing requests.
            let provider_impl: super::Provider = match transport.as_str() {
                "openrouter" => {
                    OpenRouterProvider::with_base_url(api_key.clone(), base_url.clone())
                }
                "zen" | "opencode-zen" => {
                    ZenProvider::with_base_url(api_key.clone(), base_url.clone())
                }
                "ollama" => OllamaProvider::new(),
                _ => {
                    // Generic OpenAI-compatible provider.
                    let url = base_url
                        .clone()
                        .unwrap_or_else(|| match transport.as_str() {
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
                        });

                    let (model_id, model_cfg) = models_cfg
                        .iter()
                        .next()
                        .map(|(id, mc)| (id.clone(), mc.clone()))
                        .unwrap_or_else(|| {
                            (
                                format!("{}-model", provider_name),
                                CapabilityDescriptor::default(),
                            )
                        });

                    let model = GenericOpenAIModel::new(
                        model_id,
                        provider_name.clone(),
                        url,
                        &model_cfg,
                        format!("{}/", provider_name),
                    );
                    let http_transport = super::HttpTransport::new(GENERIC_TIMEOUT)
                        .expect("failed to build generic OpenAI-compatible HTTP transport");
                    super::Provider::new(Box::new(model), Box::new(http_transport), api_key.clone())
                }
            };

            if !custom_headers.is_empty() {
                provider_impl.set_extra_headers(custom_headers.clone());
            }

            let provider: Arc<dyn ChatProvider + Send + Sync> = Arc::new(provider_impl);
            provider
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
            let provider_impl: super::Provider = match transport.as_str() {
                "openrouter" => {
                    OpenRouterProvider::with_base_url(api_key.clone(), base_url.clone())
                }
                "zen" | "opencode-zen" => {
                    ZenProvider::with_base_url(api_key.clone(), base_url.clone())
                }
                "ollama" => OllamaProvider::new(),
                _ => {
                    let url = base_url
                        .clone()
                        .unwrap_or_else(|| match transport.as_str() {
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
                        });

                    let (model_id, model_cfg) = models_cfg
                        .iter()
                        .next()
                        .map(|(id, mc)| (id.clone(), mc.clone()))
                        .unwrap_or_else(|| {
                            (
                                format!("{}-model", provider_name),
                                CapabilityDescriptor::default(),
                            )
                        });

                    let model = GenericOpenAIModel::new(
                        model_id,
                        provider_name.clone(),
                        url,
                        &model_cfg,
                        format!("{}/", provider_name),
                    );
                    let http_transport = super::HttpTransport::new(GENERIC_TIMEOUT)
                        .expect("failed to build generic OpenAI-compatible HTTP transport");
                    super::Provider::new(Box::new(model), Box::new(http_transport), api_key.clone())
                }
            };

            if !custom_headers.is_empty() {
                provider_impl.set_extra_headers(custom_headers.clone());
            }

            let with_headers: Arc<dyn ChatProvider + Send + Sync> = if custom_headers.is_empty() {
                Arc::new(provider_impl)
            } else {
                Arc::new(ProviderWithHeaders::new(
                    Arc::new(provider_impl),
                    custom_headers.clone(),
                ))
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
pub fn create_reload_target(name: &str, cfg: &ProviderConfig, api_key: String) -> ProviderTarget {
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
        std::env::set_var("TEST_RESOLVE_KEY", "sk-from-env");
        let cfg = ProviderConfig {
            api_key: Some("{env:TEST_RESOLVE_KEY}".into()),
            ..Default::default()
        };
        let key = resolve_api_key(&cfg, "test", false).unwrap();
        assert_eq!(key, "sk-from-env");
        std::env::remove_var("TEST_RESOLVE_KEY");
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
    fn test_resolve_api_key_encrypted_roundtrip() {
        use crate::security::secrets::SecretManager;
        let manager = SecretManager::new(SecretManager::generate_random_key());
        let ciphertext = manager.encrypt("sk-encrypted-secret").unwrap();
        std::env::set_var("FUSION_MASTER_KEY", manager.export_master_key_base64());

        let cfg = ProviderConfig {
            api_key_encrypted: Some(ciphertext),
            ..Default::default()
        };
        let key = resolve_api_key(&cfg, "test", false).unwrap();
        assert_eq!(key, "sk-encrypted-secret");
        std::env::remove_var("FUSION_MASTER_KEY");
    }

    #[test]
    fn test_resolve_api_key_encrypted_fails_closed_without_master_key() {
        std::env::remove_var("FUSION_MASTER_KEY");
        let cfg = ProviderConfig {
            api_key_encrypted: Some("AAAAAAAAAAAAAAAA".into()),
            ..Default::default()
        };
        let result = resolve_api_key(&cfg, "test", false);
        assert!(
            result.is_err(),
            "encrypted key without FUSION_MASTER_KEY must fail closed"
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("FUSION_MASTER_KEY"));
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
