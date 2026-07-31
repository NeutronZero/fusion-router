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

## Related ADRs

- ADR-001: Foundation architecture, Provider abstraction
- ADR-005: Three-part Provider/Model/Transport split
