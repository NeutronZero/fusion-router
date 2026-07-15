# Compiler Passes Specification

## Pass Traits

```rust
pub trait CompilerPass {
    fn name(&self) -> &str;
    fn apply(&self, ir: WorkflowIR) -> Result<WorkflowIR, CompilerError>;
}
```

All passes are **pure** — no I/O, no side effects.

## Standard Passes

### 1. ConstraintValidationPass
- Validates IR invariants (at least one node, no duplicate IDs, valid edges)
- Returns `CompilerError::ValidationError` on failure

### 2. ModelResolutionPass
- Resolves `model: None` to default model
- Applies model aliases/config overrides

### 3. BudgetOptimisationPass
- Calls `ResourceManager::can_afford` to check budget
- Downgrades models or reduces parallelism if over budget
- Placeholder in Phase 2; full implementation in Phase 4

### 4. StrategyExpansionPass (Phase 8+)
- Expands Strategy nodes into subgraphs
- Resolves strategies before lowering to ExecutionGraph

## Pass Ordering
1. ConstraintValidation
2. ModelResolution
3. BudgetOptimisation
4. StrategyExpansion (future)
