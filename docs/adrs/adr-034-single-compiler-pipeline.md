# ADR-034: Single Compiler Pipeline

- **Status:** Draft
- **Date:** 2026-08-03
- **Applies to:** compiler (`src/compiler`), server execution paths (`src/server`)
- **Charter:** `docs/implementation/security-hardening-v0.13.1.md` Phase 1, Compiler Laws 1, 2, 4, 5

## Context

The v0.13 architecture defines `WorkflowIR → Compiler passes → ExecutionGraph → Executor` as the only execution pipeline. In practice, the production HTTP execution plane (`src/server/execution.rs`) constructs `DefaultCompiler { passes: vec![] }`, bypassing constraint validation, control-flow validation, budget optimisation, and policy enforcement. Security audit finding C2 (and C5's policy denial semantics) demonstrate that the compiler boundary is currently advisory, not enforced. Policy `Deny` rules match but never block compilation.

## Decision

1. **One construction path:** `build_compiler(config) -> DefaultCompiler` is the sole production construction site for the pass pipeline. `DefaultCompiler { passes: vec![] }` is restricted to `#[cfg(test)]`.
2. **One execution contract:** every execution endpoint (chat, `/v1/executions`, triggers) compiles through the same factory. No fast path, no debug path.
3. **Policy denial is a compile error:** a matched `PolicyEffect::Deny` rule causes `PolicyCompilerPass::apply` to return `CompilerError::ValidationError`; no `ExecutionGraph` is produced for a workflow violating a matched deny rule.
4. **Capability policy is total:** `deny_list`/`allow_list` are evaluated for every capability entering the resolution result — required, optional, version-constrained, and transitive (WP 1.3).
5. **Fail-closed parsing:** unknown policy effect strings error at `PolicyIR` construction rather than defaulting to `Allow`.

## Consequences

- Compiler Laws 1, 2, 4, 5 become executable invariants backed by `tests/security_invariants.rs`.
- A workflow violating a matched Deny rule cannot produce an `ExecutionGraph` (new compiler invariant; must be added to `.memory/compiler.md` and `docs/specifications/compiler-passes.md`).
- Client-submitted workflows gain the same validation as server-built ones; previously-accepted malformed inputs now fail with `400` — expected behavior change.
- Removes the duplicated pass list in `src/server/handlers.rs`; pass order is defined once.
