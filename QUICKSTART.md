# FusionRouter Quick Start

## Prerequisites

- Rust 1.75+
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
  -d '{
    "model": "zen-7b",
    "messages": [{"role": "user", "content": "Write a Fibonacci function in Python"}]
  }'
```

## Multi-Step Workflow

FusionRouter handles complex workflows via its DAG pipeline. A "search → filter → generate" workflow:

```
User Request
  │
  ▼
[Generate: search strategy]  ──▶  [Generate: filter results]
  │                                      │
  └──"needs_search"──▶  [Generate:      │
                           deep search]──┘
```

This is produced automatically by the Planner based on the request's intent and complexity. The Compiler validates the DAG, assigns models, checks budget. The Scheduler executes nodes topologically — conditionals branch, split/join runs in parallel, loops iterate.

## Streaming

```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "zen-7b",
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
  max_daily_cost: 10.0
  max_daily_tokens: 100000
  max_concurrent: 5

strategies:
  consensus_count: 3
```

## Running Tests

```bash
cargo test                     # all tests
cargo test golden::dag         # DAG-specific tests
cargo test integration         # integration tests
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

See `docs/architecture/runtime.md` for the full pipeline description, DAG execution model, and scheduling algorithm.
