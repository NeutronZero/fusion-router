# ADR-005: Provider Abstraction

## Status
Accepted

## Context
FusionRouter must support multiple LLM providers (Zen, OpenRouter, Ollama) with different APIs and authentication.

## Decision
1. **Provider trait**: The `Provider` trait abstracts LLM interactions: `chat_completion`, `chat_completion_stream`.
2. **Three-part split** (Phase 6): `Provider` (routing), `Model` (model-specific logic), `Transport` (HTTP/stdio).
3. **Normalization layer**: Each provider adapter converts internal `ProviderRequest` to the provider's API format and normalizes the response back.
4. **Environment-based config**: API keys are read from environment variables.

## Consequences
- Providers are hot-swappable.
- New providers follow a consistent pattern.
- Streaming support is uniform across providers.
