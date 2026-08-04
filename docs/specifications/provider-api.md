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

## Tool Call Contract (v0.13.1, ADR-037 / Law 7)

- Requests: `ChatCompletionRequest.tools: Option<Vec<ToolDefinition>>` is
  serialized into the provider body as `tools` only when present (the
  executor advertises definitions only when automatic tool execution is
  enabled with a non-empty per-request allowlist).
- Responses: `ChatCompletionResponse.native_tool_calls:
  Option<Vec<ToolCall>>` (`ToolCall { id, name, arguments: Value }`),
  normalized from the provider wire shape by
  `providers::native_tool_calls_from(body, container, choice_index)`:
  - OpenAI-compatible (Zen, OpenRouter): `body["choices"][choice_index]["message"]["tool_calls"]`
    (container `"choices"`, index 0); `function.arguments` is a JSON string.
  - Ollama: `body["message"]["tool_calls"]` (container `"message"`, index -1).
- Malformed `function.arguments` strings normalize to an empty object; an
  absent/empty `tool_calls` array yields `None`.
- `native_tool_calls` is the only conduit for tool execution; model output
  text is never parsed for tool invocation.
