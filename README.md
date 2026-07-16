# FusionRouter

An intelligent, multi-provider LLM orchestration engine with DAG-based workflow planning, compilation, and execution.

Supports **linear**, **conditional branching**, **loops**, and **parallel split/join** workflows across multiple LLM providers (OpenCode Zen, OpenRouter, Ollama).

## Quick Start

```bash
cargo run
```

See [QUICKSTART.md](QUICKSTART.md) for examples, streaming, and configuration.

## Architecture

FusionRouter processes each request through a deterministic pipeline:

1. **Context Assembly** — extracts messages, files, and tools
2. **Requirements Extraction** — classifies intent and complexity
3. **Planning** — produces a WorkflowIR DAG (with conditionals, loops, split/join)
4. **Compilation** — validates, resolves models, checks budget, lowers to ExecutionGraph
5. **Scheduling** — topological execution with conditional branching, loop iteration, parallel fan-out
6. **Strategy Resolution** — expands strategies (single, consensus, reflection) into subgraphs
7. **Telemetry** — records execution evidence to SQLite

See [docs/architecture/runtime.md](docs/architecture/runtime.md) for the full pipeline and DAG execution model.

## Documentation

| Path | Content |
|------|---------|
| [QUICKSTART.md](QUICKSTART.md) | Run, test, configure |
| [docs/architecture/runtime.md](docs/architecture/runtime.md) | Pipeline & DAG execution model |
| [docs/architecture/invariants.md](docs/architecture/invariants.md) | Architectural invariants |
| [docs/specifications/workflow-ir.md](docs/specifications/workflow-ir.md) | Workflow IR spec (includes DAG nodes) |
| [docs/specifications/execution-graph.md](docs/specifications/execution-graph.md) | Execution graph spec (includes DAG nodes) |
| [docs/adr/](docs/adr/) | Architecture Decision Records (ADR-001 through ADR-006) |
