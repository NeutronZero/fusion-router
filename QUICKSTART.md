# FusionRouter Quick Start

## Prerequisites

- Rust 1.75+ (2021 Edition)
- API keys in `.env`:
  ```
  OPENCODEZEN_API_KEY=your_key
  OPENROUTER_API_KEY=your_key
  ```

## Run the Server

```bash
cargo run
```

Listens on `http://0.0.0.0:8080` by default. Config via `config/default.yaml` or `FUSION_CONFIG` env var.

## Basic Request

```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "model": "auto",
    "messages": [{"role": "user", "content": "Write a Fibonacci function in Python"}]
  }'
```

Use `"model": "auto"` to let the compiler select the optimal model based on intent and complexity. You can also specify explicit model names (e.g. `"claude-sonnet-4-20250514"`).

## Execution Intent

Control the compilation strategy via the `execution` field:

```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "model": "auto",
    "messages": [{"role": "user", "content": "Debug this crash in the auth module"}],
    "execution": {"mode": "quality"}
  }'
```

| Intent | Nodes | Behavior |
|--------|-------|----------|
| `speed` | 1 | Single Generate (fastest) |
| `balanced` | 3 | 2×Generate → Judge |
| `quality` | 5 | 3×Generate → Judge → Generate(Reflection) |
| `exhaustive` | 6 | 3×Generate → Judge → Generate(Reflection) → Judge(Consensus) |
| `constrained` | varies | Budget-aware template selection |

When omitted, complexity-based fallback selects the template automatically.

## Streaming

```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d '{
    "model": "auto",
    "stream": true,
    "messages": [{"role": "user", "content": "Count to 10"}]
  }'
```

## Configuration

Edit `config/default.yaml`:

```yaml
server:
  host: "0.0.0.0"
  port: 8080

resources:
  max_daily_cost: 100.0
  max_daily_tokens: 100000
  max_concurrent: 10
  max_concurrent_nodes: 16

strategies:
  consensus_count: 3

tools:
  allowed_shell_commands: ["ls", "echo", "cat", "cmd"]
  allowed_read_directories: ["/tmp"]
  enable_http_tool: true
  shell_timeout_secs: 30

auth:
  enabled: false
  api_keys: []

rate_limiting:
  enabled: false
```

## Running Tests

```bash
cargo test                     # all 314 tests
cargo test golden              # optimization golden tests
cargo test integration         # integration tests
cargo test strategy_sdk        # strategy SDK tests
cargo test unit                # resilience & injection tests
```

## OpenCode Integration

FusionRouter can serve as a backend provider for [OpenCode](https://opencode.ai).

### Configuration

Point OpenCode to FusionRouter by creating `~/.config/opencode/project.json`:

```bash
# Or use the setup script:
bash scripts/setup-opencode.sh
```

Or manually:

```json
{
  "provider": {
    "baseURL": "http://localhost:8080/v1",
    "apiKey": "${FUSION_ROUTER_API_KEY}"
  }
}
```

- Set `FUSION_ROUTER_API_KEY` if FusionRouter auth is enabled.
- FusionRouter handles model routing automatically; the `model` field in OpenCode is ignored.

### Setup Scripts

| Script | Platform |
|--------|----------|
| `scripts/setup-opencode.sh` | Linux / macOS / WSL |
| `scripts/setup-opencode.ps1` | Windows PowerShell |

Run the appropriate script after starting FusionRouter.

## Architecture

See [FusionRouter v0.10.0 Architecture Specification](docs/fusionrouter_architecture_v0.10.0.md) for the full pipeline, compiler design, DAG execution model, and scheduling algorithm.
