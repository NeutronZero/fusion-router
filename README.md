# FusionRouter

An intelligent, multi-provider routing layer for LLM APIs with planning, compilation, and execution capabilities.

## Quick Start

```bash
cargo run -- --config config/default.yaml
```

## Architecture

FusionRouter routes chat completion requests through a configurable pipeline:

1. **Context Assembly** — extracts messages, files, and tools from the request
2. **Requirements Extraction** — classifies intent and complexity
3. **Planning** — generates a WorkflowIR based on policies and historical evidence
4. **Compilation** — transforms IR into an executable graph via pure passes
5. **Scheduling** — executes nodes respecting dependencies, retries, and fallbacks
6. **Strategy Resolution** — resolves strategies (single, consensus, reflection) into subgraphs

## Documentation

See `docs/` for architecture, ADRs, and specifications.
