# ADR-009: Configuration Management

## Status
Accepted

## Context
FusionRouter requires flexible configuration for server settings, provider credentials, resource limits, security policies, rate limiting, logging, and tools. Configuration comes from multiple sources — YAML files, environment variables, and sensible defaults — and must be validated at startup to fail fast on misconfiguration.

## Decision

### 1. YAML-Based Config File

The primary configuration source is a YAML file loaded by `AppConfig::load(path)`:
- Deserialized with `serde_yaml` and `#[derive(Deserialize)]`
- `serde(default)` on optional sections enables incremental config files
- Config path set via `FUSION_CONFIG` env var, falling back to `config/default.yaml`
- Deny unknown fields (`serde(deny_unknown_fields)`) to catch typos

### 2. Config Structure

`AppConfig` contains these top-level sections:
- `server` — host, port, shutdown timeout, CORS config
- `resources` — max daily cost/tokens, max concurrent requests/nodes, provider limits
- `policies` — named policy rules with conditions and actions
- `providers` — per-provider base URL, API key env var, circuit breaker thresholds
- `strategies` — consensus count for FusionStrategy
- `tools` — allowed shell commands, timeout, read directories, HTTP tool toggle
- `auth` — enable/disable, api_keys list
- `rate_limiting` — enable/disable, requests per minute, burst size, cleanup interval
- `logging` — format (text/json), level, optional directory
- `model_catalog` — per-model capabilities and pricing

### 3. Environment Variable Overlay

- `dotenv::dotenv()` loads `.env` files at startup
- API keys supplied via `OPENCODEZEN_API_KEY`, `OPENROUTER_API_KEY` env vars
- Provider base URLs overrideable via provider-specific env vars (`OLLAMA_BASE_URL`, etc.)
- `FUSION_CONFIG` selects the config file path
- `RUST_LOG` controls tracing filter when not overridden by config

### 4. Startup Validation

`AppConfig::validate()` runs 11 checks before the server starts:
- Port must be > 0
- Shutdown timeout must be > 0
- Resource limits must be non-negative and non-zero where appropriate
- Auth enabled implies at least one API key configured
- Rate limiting parameters must be valid when enabled
- Logging format must be `"text"` or `"json"`, level must be non-empty

Validation failures are printed to stderr and trigger a panic with error count, preventing startup with invalid configuration.

### 5. Quota Conversion

`AppConfig::to_quota()` converts resource config into the internal `Quota` struct used by the resource manager for concurrent request tracking and provider limits.

### 6. Policy Conversion

`AppConfig::to_policies()` converts config policies into `Policy` structs used by the policy engine for request routing decisions.

## Consequences

- Single YAML file is the source of truth for operators
- Startup validation catches misconfiguration before the server binds
- Environment variables handle secrets (API keys) without committing them to config files
- The config struct evolves as new features are added, with serde defaults preserving backward compatibility
- Deny-unknown-fields prevents silent misconfiguration from typos
