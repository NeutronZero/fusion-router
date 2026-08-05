# Compiler Passes Specification

## Pass Traits

```rust
#[async_trait]
pub trait CompilerPass: Send + Sync {
    fn name(&self) -> &str;
    async fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError>;
}
```

All passes are **pure** — no I/O, no LLM calls, no side effects.

## Pipeline Construction

The pipeline is constructed exclusively via `build_compiler()` (`src/compiler/mod.rs`) — the sole production construction path (ADR-034). No production path builds `DefaultCompiler` with an empty pass list, and every execution endpoint (chat, `/v1/executions`, triggers) compiles through a compiler produced here.

The mandatory pass list is followed by an optional policy pass (appended when a `PolicyIR` is supplied), then `lower_to_graph` performs the final structural lowering to `ExecutionGraph`.

## Mandatory Passes (in order)

### 1. ConstraintValidationPass
- Rejects empty IR (at least one node required)
- Returns `CompilerError::ValidationError` on failure

### 2. ControlFlowValidationPass
- Validates every edge references known source/target nodes
- Enforces control-flow node shape: Conditional needs ≥1 outgoing edge with a condition; Loop needs ≥1 outgoing edge and `max_iterations` in config; Split needs ≥2 outgoing edges; Join needs ≥2 incoming edges; Barrier needs ≥1 incoming and ≥1 outgoing edge
- Detects illegal cycles via three-color DFS (loop back-edges exempt)

### 3. ModelResolutionPass
- Fills `model: None` from the `ModelCatalog` based on requirements (tools or high coding score → code model, high reasoning score → architecture model, else fast)
- Never overrides an explicitly supplied model

### 4. BudgetOptimisationPass
- Calls `ResourceManager::can_afford` against estimated cost/tokens
- Returns a compile error when the budget is exceeded

## Optional Pass

### 5. PolicyCompilerPass (appended when PolicyIR is supplied)
- Applies the compiled `PolicyIR` to the IR
- A matched Deny policy rule blocks compilation: no `ExecutionGraph` can be produced (ADR-034 / Law 2)

## Lowering

After the pass loop, `lower_to_graph` (`src/compiler/mod.rs`) performs a **direct structural lowering** of `WorkflowIR` → `ExecutionGraph`:

- 1:1 mapping of IR node kinds to execution node kinds
- Strategy carried through as a field on each node
- `primitive_graph_hash` is set to 0 — no `PrimitiveGraph` is produced on the live path

**Strategy expansion happens at compile time.** `strategy_expansion` (`src/compiler/strategy_expansion.rs`) runs after `lower_to_graph`: it looks each strategy node up in the default `StrategyRegistry`, calls `Strategy::lower` → `PrimitiveGraph::to_execution_graph`, and attaches the deterministic result to `node.subgraph`. The executor's `resolve_strategy` consumes `node.subgraph` verbatim; runtime lowering survives only as a legacy fallback for graphs compiled before expansion existed.

## Status

- Phase 1 (v0.13.1): the four mandatory passes + optional policy pass are implemented and law-tested (`law1_build_compiler_*`, `law2_deny_blocks_compilation`, `law5_execution_plane_uses_full_passes`).
- The optimization framework in `src/compiler/optimization` (`DeadNodeEliminationPass`, `FanOutConsolidationPass`) operates on `PrimitiveGraph` and is **not wired into the production pipeline**; it is exercised only by its own unit tests.
