# FusionRouter

**Version:** 0.14.5 · **License:** MIT OR Apache-2.0

Compiler-driven multi-model orchestration engine. Requests become a planned workflow IR, pass through a deterministic compiler, then execute on a DAG scheduler with provider dispatch, policy gates, budgets, and fail-closed tool use.

```text
HTTP (OpenAI / Anthropic compatible)
  → context + requirements
  → fusion-planner   (snapshot-driven WorkflowIR)
  → fusion-compiler  (validate · strategy lower · dead-node · model · budget · policy)
  → fusion-scheduler (DAG / WorkQueue)
  → fusion-runtime   (providers · retry · tools · subgraphs)
  → response (+ optional SSE transport)
```

Architecture is **converged**: authoritative logic lives under `crates/`. The host binary (`src/`) is adapters, providers, HTTP, and control-plane wiring. Enforced by `scripts/check_monolith_freeze.py` (11 gates).

---

## Quick start

### Prerequisites

- Rust stable (edition 2021)
- Optional: provider API keys (`OPENAI_API_KEY`, `OPENROUTER_API_KEY`, etc.)

### Build & run

```bash
cp .env.example .env   # set provider keys as needed
cargo build --release
cargo run --release
```

Default HTTP server binds according to config under `config/`.

### Tests

```bash
cargo test --workspace
python3 scripts/check_monolith_freeze.py   # must print ARCHITECTURE STATUS: CONVERGED
```

### Docker

```bash
docker build -t fusion-router .
docker run --env-file .env -p 8080:8080 fusion-router
```

---

## Workspace layout

| Path | Role |
|------|------|
| `crates/fusion-ir` | Provider-free planning IR |
| `crates/fusion-types` | Execution types, graphs, shared values |
| `crates/fusion-planner` | Snapshot-driven planning |
| `crates/fusion-compiler` | Pass pipeline, strategy expansion, scoring |
| `crates/fusion-scheduler` | Concurrent DAG scheduling |
| `crates/fusion-runtime` | Node execution, tools, subgraphs |
| `crates/fusion-core` / `fusion-kernel` | Errors, NanoUSD, resource contracts |
| `crates/fusion-placement` / `fusion-worker*` | Distributed runtime direction (v0.15) |
| `src/` | Host binary: HTTP, providers, policy registry, bridges |
| `tests/` | E2E golden, contract wiring, security, release gates |
| `docs/` | ADRs, architecture, debt register, roadmaps |
| `scripts/check_monolith_freeze.py` | Architectural convergence firewall |

---

## Compiler pipeline (production)

```text
constraint_validation
control_flow_validation
strategy_lowering
dead_node_elimination
model_resolution
budget_optimisation
[+ policy]                 # deny = compile error (Law 2)
→ lower_to_graph           # attaches strategy subgraphs; sets content hash
```

Streaming and non-streaming chat requests share this pipeline; SSE is a transport adapter over the completed result (Gate 08).

---

## Architectural invariants (selected)

1. **Immutable IR / graph** after construction  
2. **Deterministic compilation** — identical inputs → identical canonical graphs  
3. **Planner isolation** — no direct provider calls from the planner  
4. **Single source of truth** — business logic in `crates/`, not duplicated in `src/`  
5. **NanoUSD** for internal monetary accounting  
6. **Single PolicyRegistry + frozen CapabilityRegistry** in `AppState`  
7. **Fail-closed tools** (ADR-037) — allowlist + auto-exec gates  

Full list: [`docs/architecture/invariants.md`](docs/architecture/invariants.md)

---

## Documentation

| Document | Location |
|----------|----------|
| Architecture status | [`docs/architecture/status.md`](docs/architecture/status.md) |
| Specification | [`docs/architecture/specification.md`](docs/architecture/specification.md) |
| Debt register | [`docs/architecture/architecture_debt_register.md`](docs/architecture/architecture_debt_register.md) |
| ADRs | [`docs/adr/`](docs/adr/) |
| Roadmap v0.15 (distributed) | [`docs/architecture/v0.15_distributed_architecture.md`](docs/architecture/v0.15_distributed_architecture.md) |

---

## Development notes

- Prefer extending crates over adding host-side execution logic.
- Run the freeze script before merging architecture-sensitive changes.
- Strategy expansion and execution belong in `fusion-compiler` / `fusion-runtime`.
- Host `src/strategies` remains for plugin/ABI descriptors; the production execute path delegates to crates.

### Useful commands

```bash
cargo test -p fusion-compiler
cargo test -p fusion-runtime
cargo test --test e2e_golden
cargo test --test contract_wiring
python3 scripts/check_monolith_freeze.py
```

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
