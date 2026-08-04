# ADR-001: Compiler Pipeline Architecture

## Status
Accepted (AF-003 Frozen)

## Context
FusionRouter requires a deterministic, highly optimized pipeline to translate intent into an executable `ExecutionGraph` DAG without runtime overhead or provider-specific heuristics.

## Decision
Implement a multi-pass compiler pipeline inside `fusion-compiler`:
`WorkflowIR -> Validation -> Capability Resolution -> Constraint Solver -> Constant Folding -> Dead Node Elimination -> Node Fusion -> Retry Injection -> Fallback Injection -> Scheduling Hints -> Execution Graph`.

## Alternatives Considered
- Direct execution of dynamic workflows without graph lowering: Rejected due to lack of explainability, inability to dry-run simulate, and non-deterministic execution paths.
- Single-pass lowering: Rejected because pass isolation allows independent benchmark validation and modular compiler optimization passes.

## Consequences
- All execution paths funnel through compiled `ExecutionGraph` instances.
- Passes are 100% deterministic (Invariant 3).
- Enables multi-dimensional `Explain Route` scoring and dry-run simulation mode.
