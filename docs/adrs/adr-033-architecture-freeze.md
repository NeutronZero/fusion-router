# ADR-033: v0.13 Architecture Freeze — Six Core Abstractions

- **Status:** Accepted (Frozen)
- **Date:** 2026-07-31
- **Applies to:** entire architecture

## Context

FusionRouter v0.12 delivered the capability platform (runtime, package, operations, host interface, developer platform). The architecture is now declared frozen at v0.13.0. The project identity is defined by six stable abstractions, and all future development must integrate through them rather than introduce new foundational concepts.

## Decision

Freeze the six core abstractions as stable public contracts:

1. `NormalizedIntent` — canonical provider-free goals and constraints (`src/intent`)
2. `WorkflowIR` — provider-independent logical workflow (`src/ir`)
3. `ExecutionAbi` — executable workflow contract emitted only by the compiler (`src/abi`)
4. `ExecutionTarget` — provider-independent runtime placement constraints (`src/target`)
5. `ExecutionRuntimeInterface` — runtime execution contract (`src/eri`)
6. `CapabilityRegistry` + `CapabilityTrait` — semantic execution capabilities (`src/capability`, `crates/fusion-plugin-api`)

Architectural laws from the v0.13 specification are binding: provider independence above runtime (laws 7, 8), only the compiler generates an ABI (law 4), every ABI targets an explicit execution target (law 5), optimizations preserve semantic equivalence (law 9).

## Consequences

- Provider/model/endpoint references above the Runtime layer are architectural drift and must be removed incrementally (v0.14 boundary reconciliation per the reconciliation design).
- Changes to the six contracts after this freeze require a new ADR.
- v0.12 implementation details may live behind compatibility adapters but cannot redefine the contracts.
