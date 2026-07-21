# FusionRouter Rules & Guidelines

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
