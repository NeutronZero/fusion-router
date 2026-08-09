# FusionRouter User Guide

Welcome to **FusionRouter**, the compiler-driven AI orchestration platform.

## Getting Started

1. **Launch the server:** Run `cargo run -p fusion-router`.
2. **Send a request:** POST to `/v1/chat/completions` with your prompt.
3. **Check health:** GET `/v1/health` for platform status.

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/chat/completions` | POST | Send a prompt through the compiler pipeline |
| `/v1/executions` | GET | List execution history |
| `/v1/health` | GET | Platform health check |
| `/v1/ready` | GET | Readiness probe |

## Architecture

```
Request → Planner → Compiler → Scheduler → Executor → Providers
```

Every request passes through the full compiler pipeline (11 mandatory passes) before reaching providers.
