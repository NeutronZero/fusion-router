# FusionRouter Rules & Guidelines

## 🏛️ Architecture Memory

This repository has a curated architecture handbook in `.memory/`. Always read it before exploring source code.

### Session Bootstrap (read these first)
1. `.memory/architecture-map.md` — one-page system pipeline map
2. `.memory/overview.md` — project vision, module catalog, invariants
3. `.memory/editing-guide.md` — edit boundaries, common tasks, safety rules

### Consult on Demand
- `.memory/architecture.md` — full system design, invariants, data flow
- `.memory/compiler.md` — compiler passes, IR, optimization
- `.memory/runtime.md` — DAG execution, node model, scheduling
- `.memory/planner.md` — planner types, WorkflowIR generation
- `.memory/capability-system.md` — capability registry, SDK, plugin API
- `.memory/execution.md` — execution model, sessions, replay
- `.memory/providers.md` — provider abstraction, transports, adapters
- `.memory/scheduler.md` — scheduler, work queue, distributed
- `.memory/policies.md` — policy compilation, release gates, attestation
- `.memory/telemetry.md` — events, metrics, tracing, audit
- `.memory/plugin-system.md` — extension points, WASM, ABI
- `.memory/roadmap.md` — version milestones
- `.memory/glossary.md` — terminology
- `.memory/adrs.md` — ADR index
- `.memory/module-index.md` — component directory

### Validation
- Architecture changes are incomplete until `.memory/` is updated.
- Run `python scripts/check-memory.py` before committing architecture changes.

---

## 🛠️ Build & Test Commands
- **Check code**: `cargo check`
- **Run default tests**: `cargo test`
- **Run all feature tests**: `cargo test --all-features`
- **Check bare library**: `cargo check --no-default-features --lib`
- **Run benchmarks**: `cargo bench`

---

## 📦 Build Artifacts & Cargo Clean
- **`cargo clean` Usage**: ONLY run `cargo clean` when a full recompilation is strictly required (e.g., switching toolchains, cross-compiling, or changing target profiles).
- **Routine Builds**: NEVER run `cargo clean` after routine `build`/`test` loops or for everyday disk management. Doing so destroys cached dependency artifacts and forces unnecessary recompilations.

---

## ⚡ Feature Flags & Performance
- Heavy or non-essential dependencies MUST remain feature-gated:
  - `semantic-cache`: Gates `usearch` and semantic caching modules (`#[cfg(feature = "semantic-cache")]`).
  - `prometheus-metrics`: Gates Prometheus metric collection.
- Test-only dependencies belong strictly in `[dev-dependencies]`.
- Always verify changes compile both WITH and WITHOUT optional default features enabled.

---

## 🎯 Code Quality & Standards
- **Zero Warnings**: Keep the build clean. Remove dead code, unused imports, or unreferenced parameters immediately.
- **Intentional Stubs**: If code is intentionally kept for future use, annotate it with `#[allow(dead_code)]` and include a brief explanatory comment.
- **Atomic Commits**: Prefer small, logical commits with concise conventional commit messages (e.g., `feat:`, `fix:`, `chore:`).
- **Public API Stability**: Do not alter public API signatures without explicit approval.
