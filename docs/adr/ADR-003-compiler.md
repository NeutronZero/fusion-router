# ADR-003: Compiler

## Status
Accepted

## Context
The Compiler transforms WorkflowIR into an ExecutionGraph. It must be deterministic, pure, and testable.

## Decision
1. **Pass pipeline**: The compiler is a pipeline of pure `CompilerPass` implementations.
2. **Pure passes**: Each pass is a pure function: `(IR) -> Result<IR, Error>`. No side effects, no I/O.
3. **Deterministic**: Given the same IR and config, output is always identical.
4. **Lowering**: The final step lowers IR to ExecutionGraph (concrete model names, resolved retry policies, etc.).
5. **Passes**: Initial passes include ConstraintValidation, ModelResolution, and BudgetOptimisation.

## Consequences
- Easy to test (golden tests for each pass).
- Passes can be composed, reordered, or disabled.
- Serialization of IR at each stage enables debugging.
