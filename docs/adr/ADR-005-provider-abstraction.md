# ADR-005: Provider/Model/Transport Split

## Status
Accepted (updated for Phase 6)

## Context
FusionRouter must support multiple LLM providers (OpenCode Zen, OpenRouter, Ollama) with different APIs, capabilities, pricing, and authentication.

## Decision
1. **Three-part split**: The monolithic `Provider` trait is split into three concerns:
   - `Model` trait — model identity, capabilities, pricing, request formatting, response normalization
   - `Transport` trait — low-level HTTP/stdio communication, error handling
   - `Provider` struct — composes `Box<dyn Model>` + `Box<dyn Transport>`, implements `ChatProvider` trait
2. **`ChatProvider` trait** — the public-facing interface (`chat_completion`), implemented by `Provider` and `ProviderRouter`
3. **`TransportRequest`/`TransportResponse`** — intermediate types decoupling model formatting from transport
4. **Model implementations**: `zen_model.rs`, `openrouter_model.rs`, `ollama_model.rs` each implement `Model::format_request` and `Model::normalize_response`
5. **Transport implementations**: `src/transport/http.rs` (`HttpTransport`) for HTTP, stdio planned for future
6. **Convenience wrappers**: `zen.rs`, `openrouter.rs`, `ollama.rs` provide `new()` functions that wire `Model` + `Transport` into a `Provider`
7. **Environment-based config**: API keys from `OPENCODEZEN_API_KEY`, `OPENROUTER_API_KEY`, `OLLAMA_BASE_URL`

## Consequences
- Models and transports are independently testable and swappable.
- Adding a new provider = implement `Model` + pick a `Transport`.
- Streaming normalization is consistent across providers.
- The `ChatProvider` trait provides a stable public API.
