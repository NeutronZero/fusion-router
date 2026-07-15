# Provider API Specification

## Traits

### Provider (Phase 0-5)
- `chat_completion(request) -> Response`
- `chat_completion_stream(request) -> Stream<String>`
- `name() -> &str`

### Model (Phase 6+)
- `model_name() -> &str`
- `provider_name() -> &str`
- `generate(request) -> Response`
- `generate_stream(request) -> Stream<String>`

### Transport (Phase 6+)
- `send(request) -> Response`
- `send_stream(request) -> Stream<String>`

## Adapter Implementations

- **Zen**: HTTP to api.zenprovider.com, OpenAI-compatible format
- **OpenRouter**: HTTP with unified API for multiple models
- **Ollama**: HTTP or stdio for local models
