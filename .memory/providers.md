# FusionRouter Providers

## Overview

The provider abstraction layer decouples FusionRouter from specific LLM APIs. All LLM interactions go through the `Provider` trait, enabling transparent routing, circuit breaking, and multi-model support.

**Location:** `src/providers/`, `src/transport/`
**Design doc:** `docs/specifications/provider-api.md`

## Architecture (ADR-005)

Three-part split:

```
┌─────────────────────────────────────────┐
│              Provider                   │
│  (Composes Model + Transport)           │
├─────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐    │
│  │    Model     │  │  Transport   │    │
│  │  (LLM logic) │  │  (HTTP/WSS)  │    │
│  └──────────────┘  └──────────────┘    │
└─────────────────────────────────────────┘
```

## Traits

| Trait | Purpose |
|-------|---------|
| `Provider` | Composes Model + Transport, provides unified LLM interface |
| `Model` | LLM-specific behavior (prompt formatting, response parsing) |
| `Transport` | Wire protocol (HTTP, WebSocket, Stdio) |

## Provider Infrastructure

| Component | File | Purpose |
|-----------|------|---------|
| `ProviderRegistry` | `src/providers/registry.rs` | Provider registration and lookup |
| `ProviderRouter` | `src/providers/router.rs` | Routes requests to available providers |
| `CircuitBreaker` | `src/providers/circuit_breaker.rs` | 3-state circuit breaker (Closed/Open/Half-Open) |
| `CircuitBreakingProvider` | `src/providers/circuit_breaking_provider.rs` | Provider wrapper with circuit breaking |

### Circuit Breaker States

- **Closed** — Normal operation, requests pass through
- **Open** — Fail fast, no requests forwarded
- **Half-Open** — Probing: allow limited requests to test recovery

## Transport Implementations

| Transport | File | Features |
|-----------|------|----------|
| HTTP | `src/transport/http.rs` | reqwest-based, SSE streaming support |
| WebSocket | `src/transport/websocket.rs` | WebSocket transport (tokio-tungstenite) |
| Stdio | `src/transport/stdio.rs` | Subprocess stdio for local models |
| Backoff | `src/transport/backoff.rs` | Exponential backoff strategy |

Transports are constructed with per-provider timeouts:
- `new_openrouter_provider` uses 600 s (free streaming requests keep long generations; 30 s previously caused node failures)
- `new_zen_provider` uses 300 s

### HTTP Transport Semantics

- **Client build failures fail fast**: `HttpTransport::new`/`with_backoff`
  return `Result`; a failed `Client` builder (TLS/proxy misconfig) aborts
  provider construction instead of silently replacing the client with a
  default that has **no request timeout** (`transport/http.rs`).
- **Retry policy**: only transient failures are retried — HTTP 429, 5xx,
  network and serialization errors — with exponential backoff capped at
  `max_retries` (default 5). Permanent 4xx client errors fail immediately.
- **Prefix-stripping contract**: the registry routes on `<provider-key>/`
  prefixes; each `*_model::format_request` must strip its own registered
  prefix before forwarding (`zen/`, `opencode/`, and `openrouter/` are
  stripped today) so upstream APIs receive bare model ids.

## Model Adapters

| Adapter | File | Provider |
|---------|------|----------|
| OpenRouter | `src/providers/openrouter.rs` | OpenRouter API (multi-model gateway) |
| OpenRouterModel | `src/providers/openrouter_model.rs` | OpenRouter model config |
| Zen | `src/providers/zen.rs` | Zen API |
| ZenModel | `src/providers/zen_model.rs` | Zen model config |
| Ollama | `src/providers/ollama.rs` | Local Ollama |
| OllamaModel | `src/providers/ollama_model.rs` | Ollama model config |

## Key Invariants

- All LLM interactions go through `Provider` trait
- Circuit breaker prevents cascading failures
- Provider selection can be policy-influenced
- Transport abstractions enable heterogeneous backends
- **Native tool calls (Law 7 / ADR-037):** request bodies include `tools`
  definitions only when present on `ChatCompletionRequest`; responses
  normalize `choices[0].message.tool_calls` (OpenAI shape) or
  `message.tool_calls` (Ollama shape) into typed
  `ChatCompletionResponse.native_tool_calls` via `native_tool_calls_from`
  (arguments strings are JSON-parsed into structured values).

## Related ADRs

- ADR-001: Foundation architecture, Provider abstraction
- ADR-005: Three-part Provider/Model/Transport split
