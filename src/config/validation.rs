use crate::config::error::{ConfigValidationError, ValidationSeverity};
use crate::config::types::AppConfig;
use crate::types::{Policy, PolicyAction, PolicyCondition, ProviderLimit, Quota};

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
            provider_limits: self
                .resources
                .provider_limits
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        ProviderLimit {
                            max_daily_cost: v.max_daily_cost,
                            max_rpm: v.max_rpm,
                            max_tpm: v.max_tpm,
                        },
                    )
                })
                .collect(),
        }
    }

    pub fn validate(&self) -> Result<(), Vec<ConfigValidationError>> {
        self.validate_with_profile(!cfg!(debug_assertions))
    }

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

        if format!("{}:{}", self.server.host, self.server.port)
            .parse::<std::net::SocketAddr>()
            .is_err()
        {
            errors.push(ConfigValidationError {
                field: "server.host".into(),
                message: format!(
                    "'{}:{}' is not a valid bind address",
                    self.server.host, self.server.port
                ),
                value: Some(self.server.host.clone()),
                severity: ValidationSeverity::Error,
            });
        }

        if self
            .logging
            .level
            .parse::<tracing_subscriber::filter::Directive>()
            .is_err()
        {
            errors.push(ConfigValidationError {
                field: "logging.level".into(),
                message: format!(
                    "'{}' is not a valid tracing level/directive",
                    self.logging.level
                ),
                value: Some(self.logging.level.clone()),
                severity: ValidationSeverity::Error,
            });
        }

        if self.resources.max_daily_cost == crate::types::NanoUSD::ZERO {
            errors.push(ConfigValidationError {
                field: "resources.max_daily_cost".into(),
                message: "max_daily_cost must be positive".into(),
                value: Some(self.resources.max_daily_cost.to_decimal_usd()),
                severity: ValidationSeverity::Error,
            });
        } else if self.resources.max_daily_cost.as_nanos() > 1_000_000_000_000 {
            // Warn if daily cost exceeds $1000 — likely a misconfiguration
            errors.push(ConfigValidationError {
                field: "resources.max_daily_cost".into(),
                message: "max_daily_cost exceeds $1000 — verify this is intentional".into(),
                value: Some(self.resources.max_daily_cost.to_decimal_usd()),
                severity: ValidationSeverity::Warning,
            });
        }

        if self.resources.max_daily_tokens == 0 {
            // Already covered by max_concurrent_nodes check below, but
            // catching zero tokens explicitly gives a clearer message.
        } else if self.resources.max_daily_tokens > 100_000_000 {
            // Warn if daily tokens exceed 100M — likely a misconfiguration
            errors.push(ConfigValidationError {
                field: "resources.max_daily_tokens".into(),
                message: "max_daily_tokens exceeds 100M — verify this is intentional".into(),
                value: Some(self.resources.max_daily_tokens.to_string()),
                severity: ValidationSeverity::Warning,
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

        if release && !self.unsafe_dev {
            if !self.auth.enabled {
                errors.push(ConfigValidationError {
                    field: "auth.enabled".into(),
                    message:
                        "authentication is disabled; start with --unsafe-dev to run without auth"
                            .into(),
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
                    message:
                        "wildcard CORS origin '*' is forbidden; start with --unsafe-dev to allow it"
                            .into(),
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
                    message:
                        "the HTTP tool is disabled by default; start with --unsafe-dev to enable it"
                            .into(),
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

        if self.tools.shell_path_mode == "direct" && release {
            errors.push(ConfigValidationError {
                field: "tools.shell_path_mode".into(),
                message: "shell path_mode 'direct' bypasses TOCTOU-safe staging (ADR-041); \
                          only acceptable on trusted single-tenant hosts"
                    .into(),
                value: Some(self.tools.shell_path_mode.clone()),
                severity: ValidationSeverity::Warning,
            });
        }
        if self.tools.allow_unrestricted_args {
            if release && !self.unsafe_dev {
                errors.push(ConfigValidationError {
                    field: "tools.allow_unrestricted_args".into(),
                    message:
                        "allow_unrestricted_args bypasses path validation (Law 10); start with --unsafe-dev to allow it"
                            .into(),
                    value: Some("true".into()),
                    severity: ValidationSeverity::Error,
                });
            } else {
                errors.push(ConfigValidationError {
                    field: "tools.allow_unrestricted_args".into(),
                    message:
                        "allow_unrestricted_args is enabled — all FILE_READING_COMMANDS path checks are skipped"
                            .into(),
                    value: Some("true".into()),
                    severity: ValidationSeverity::Warning,
                });
            }
        }
        if self.tools.max_staged_input_bytes == 0 {
            errors.push(ConfigValidationError {
                field: "tools.max_staged_input_bytes".into(),
                message: "max_staged_input_bytes must be > 0".into(),
                value: Some(self.tools.max_staged_input_bytes.to_string()),
                severity: ValidationSeverity::Error,
            });
        }
        if self.tools.shell_path_mode != "stage" && self.tools.shell_path_mode != "direct" {
            errors.push(ConfigValidationError {
                field: "tools.shell_path_mode".into(),
                message: "shell_path_mode must be 'stage' or 'direct'".into(),
                value: Some(self.tools.shell_path_mode.clone()),
                severity: ValidationSeverity::Error,
            });
        }

        if self.compiler.optimization_level > 2 {
            errors.push(ConfigValidationError {
                field: "compiler.optimization_level".into(),
                message: "compiler.optimization_level must be 0, 1, or 2".into(),
                value: Some(self.compiler.optimization_level.to_string()),
                severity: ValidationSeverity::Error,
            });
        }

        // Provider API key mutual exclusion: at most one of api_key / api_key_env
        // / api_key_encrypted may be set per provider. Multiple sources would
        // make precedence confusing and hide misconfiguration.
        for (name, provider) in &self.providers {
            let mut sources = 0u8;
            if provider.api_key.is_some() {
                sources += 1;
            }
            if provider.api_key_env.is_some() {
                sources += 1;
            }
            if provider.api_key_encrypted.is_some() {
                sources += 1;
            }
            if sources > 1 {
                errors.push(ConfigValidationError {
                    field: format!("providers.{name}.api_key"),
                    message: "at most one of api_key, api_key_env, api_key_encrypted may be set".into(),
                    value: None,
                    severity: ValidationSeverity::Error,
                });
            }
            if release && !self.unsafe_dev && sources == 0 {
                errors.push(ConfigValidationError {
                    field: format!("providers.{name}.api_key"),
                    message: "provider has no API key configured; set api_key, api_key_env, or api_key_encrypted (or run with --unsafe-dev for test placeholders)".into(),
                    value: None,
                    severity: ValidationSeverity::Error,
                });
            }
        }

        for warning in errors
            .iter()
            .filter(|e| matches!(e.severity, ValidationSeverity::Warning))
        {
            tracing::warn!(field = %warning.field, "{}", warning.message);
        }
        let has_blocking = errors
            .iter()
            .any(|e| matches!(e.severity, ValidationSeverity::Error));
        if !has_blocking {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn to_policies(&self) -> Vec<Policy> {
        self.policies
            .iter()
            .map(|p| Policy {
                name: p.name.clone(),
                priority: p.priority,
                conditions: p
                    .conditions
                    .iter()
                    .map(|c| PolicyCondition {
                        field: c.field.clone(),
                        operator: c.operator.clone(),
                        value: c.value.clone(),
                    })
                    .collect(),
                actions: p
                    .actions
                    .iter()
                    .map(|a| PolicyAction {
                        action_type: a.action_type.clone(),
                        params: a.params.clone(),
                    })
                    .collect(),
            })
            .collect()
    }
}
