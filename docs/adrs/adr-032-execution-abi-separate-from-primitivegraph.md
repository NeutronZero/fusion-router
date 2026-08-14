# ADR-032: Execution ABI Is Generated Separately from PrimitiveGraph

- **Status:** Accepted
- **Date:** 2026-07-31
- **Applies to:** Compiler Core (v0.14), Execution ABI contract

## Context

`PrimitiveGraph` (`src/compiler/ir/primitive_ir.rs`) is currently documented as "the formal Runtime ABI contract between compiler and scheduler" and carries provider-specific fields (`PrimitiveNodeKind::LLMGenerate { model, .. }`). The frozen v0.13 architecture defines Execution ABI as a separate stable, versioned, runtime-independent contract that "represents executable work rather than logical work", and states that only the compiler may generate it.

Two options exist:

- **Option A:** Promote `PrimitiveGraph` to the public Execution ABI.
- **Option B:** Keep `PrimitiveGraph` compiler-internal and emit a separate `ExecutionAbi` from an ABI generator stage.

## Decision

**Option B.** `PrimitiveGraph` remains compiler-internal lowered IR. A new `src/abi` module defines the frozen `ExecutionAbi` contract, produced only by the compiler's ABI generator stage. The `src/abi` contract is provider-free: nodes reference capabilities, never models.

## Consequences

- Compiler internals (`PrimitiveGraph`, strategy IR) can evolve without breaking the runtime contract.
- `ExecutionAbi` can be versioned and kept backward-compatible independently of optimization passes.
- A translation layer (ABI generator) is required between `PrimitiveGraph` and `ExecutionAbi`; this is v0.14 work.
- The doc comment on `PrimitiveGraph` claiming it is the runtime ABI must be corrected when the ABI generator lands (v0.14), not in this milestone.
