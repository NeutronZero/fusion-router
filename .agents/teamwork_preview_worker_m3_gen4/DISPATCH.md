# Dispatch Assignment: Milestone 3 (Code Quality & Clippy Warnings)

## Mission
Fix all 31 clippy warnings and dead code/compiler warnings across the repository so that `cargo clippy --all-targets --all-features -- -D warnings` and `cargo check --all-targets` compile cleanly with 0 warnings.

## Target Files
All `src/` and `tests/` files as needed (excluding files modified by M1 and M2: `src/main.rs`, `src/tools/shell_tool.rs`, `src/config/mod.rs`, `config/default.yaml`, `src/middleware/rate_limit.rs`, `tests/security.rs`, `tests/integration/opencode.rs`).

## Detailed Instructions
1. Read `c:\Projects\fusion-router\.agents\ORIGINAL_REQUEST.md` and survey report `c:\Projects\fusion-router\.agents\teamwork_preview_explorer_survey_3\handoff.md`.
2. Run `cargo clippy --all-targets --all-features -- -D warnings` to observe all clippy errors.
3. Fix all clippy errors across the codebase. Key areas identified:
   - `src/planner/mod.rs`: rename or implement `FromStr` trait for `from_str`.
   - `src/compiler/passes/legacy_passes.rs`: simplify `map_or`.
   - `src/compiler/optimization/mod.rs`: add `Default` for `DeadNodeEliminationPass`.
   - `src/scheduler/default.rs`: replace redundant pattern matching with `.is_err()`.
   - `src/executor/mod.rs`: simplify `map_or`.
   - `src/providers/zen_model.rs`: rewrite block using `?` operator.
   - `src/providers/ollama.rs`: fix `new` returning `Self`.
   - `src/providers/mod.rs`: refactor loop to `while let`.
   - `src/transport/backoff.rs`: rename `next` or implement `Iterator`.
   - `src/types/anthropic.rs`: replace `.filter_map` with `.map`.
   - `src/devex/commands/build.rs`: simplify `map_or`.
   - `src/release/gate.rs`: rename or implement `FromStr` trait.
   - `src/feature_gate/mod.rs`: remove needless borrows.
   - `src/events/consumers/checkpoint.rs`: replace manual modulo check with `.is_multiple_of()`.
   - All other clippy lints across `src/` and `tests/`.
4. Fix compiler warnings (dead code, unused fields, unused functions, unused imports) where appropriate.
5. Verify `cargo check --all-targets`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features`.
6. Write `handoff.md` in your working directory `c:\Projects\fusion-router\.agents\teamwork_preview_worker_m3_gen4` detailing changes, test commands, and output.

## MANDATORY INTEGRITY WARNING
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## 2026-08-03T16:19:42Z
You are worker_m3_gen4 (role: teamwork_preview_worker).
Your working directory is c:\Projects\fusion-router\.agents\teamwork_preview_worker_m3_gen4.
Please read c:\Projects\fusion-router\.agents\teamwork_preview_worker_m3_gen4\DISPATCH.md for your instructions.

Scope:
1. Fix all 31 clippy warnings across src/ and tests/ so that cargo clippy --all-targets --all-features -- -D warnings passes cleanly with 0 warnings.
2. Fix dead code / compiler warnings (unused fields, unused functions, unused imports) across src/ so that cargo check --all-targets has 0 warnings.
3. Do NOT edit src/main.rs, src/tools/shell_tool.rs, src/config/mod.rs, config/default.yaml, src/middleware/rate_limit.rs, tests/security.rs, or tests/integration/opencode.rs (these are modified by M1 and M2).
4. Run cargo check --all-targets, cargo clippy --all-targets --all-features -- -D warnings, and cargo test --all-features to verify.
5. Update graphify index if applicable (graphify update .).
6. Write handoff.md in c:\Projects\fusion-router\.agents\teamwork_preview_worker_m3_gen4 detailing changes and test results, then report completion via send_message to parent.

