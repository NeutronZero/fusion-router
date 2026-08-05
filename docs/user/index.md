# FusionRouter User Guide

Welcome to **FusionRouter**, the compiler-driven AI orchestration platform.

## Two Binaries

| Binary | What it is | When to use |
|--------|-----------|-------------|
| `fusion-router` (monolith, `src/`) | **Production request path** — HTTP server, planner, compiler, scheduler, executor, providers | Real API usage (`/v1/chat/completions`, `/v1/executions`) |
| `fusion-server` (Studio, `apps/`) | **Simulation-only sandbox** — serves the Studio UI with placeholder data; no scheduler, executor, or providers behind it | UI development / visual demos |

The Studio stack (`apps/fusion-server`, `fusion-studio-api`, and the `fusion-*` workspace crates) is a **SIMULATION**: provider health, latencies, scores, dashboards, and chat replies are hardcoded for UI development. Every Studio API response carries `"simulation": true`. It is NOT the production request path, and its default port (8787) is distinct from the monolith's (8080) so both can run side by side.

## Getting Started in 5 Minutes

1. **Launch the production server:** Run `fusion-router` (or `cargo run -p fusion-router`).
2. **Launch the Studio sandbox (optional):** Run `fusion-server` (or `cargo run -p fusion-server`), then open `http://localhost:8787` (override with the `FUSION_STUDIO_PORT` env var).
3. **First-Run Setup Wizard (Studio sandbox only):**
   - Select your primary provider (OpenRouter, Anthropic, OpenAI, or Ollama).
   - Enter your API Key credentials (encrypted via AES-256-GCM).
   - Test Connection to verify latency (simulated).
   - Auto-discover local models (Ollama on `11434`, LM Studio on `1234`).
4. **Send Your First Verification Chat Prompt:** In the Studio sandbox, chat replies are simulated placeholders.

## Fusion Studio Features
- **Provider Management:** Hot-reload keys and settings without server restart (simulated).
- **Compiler Inspector:** Click any chat message to open the 5-Tab Compiler Inspector and view why providers were selected (scores are simulated).
- **Mission Control Dashboard:** View live executions, daily costs, and system status (simulated).
