# Transport API Specification

Transport handles the low-level communication with LLM providers.

## Implementations

### HTTP Transport
- Uses `reqwest` client
- Supports streaming via Server-Sent Events
- Configurable timeouts, retries, headers

### Stdio Transport (future)
- Spawns a local process
- Communicates via stdin/stdout
- Used for local models (Ollama, llama.cpp)

## Request Format
- `model`: Model identifier
- `messages`: Array of {role, content}
- `temperature`: Optional sampling temperature
- `max_tokens`: Optional max completion tokens
- `stream`: Boolean for streaming mode
