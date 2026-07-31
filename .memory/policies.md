# FusionRouter Policy System

## Overview

FusionRouter has a dual policy system: **compilation policies** that influence graph construction and **release governance policies** that gate production deployments.

**Location:** `src/policy/` (compilation), `src/release/` (governance)

## Compilation Policy (`src/policy/`)

### Components

| Component | File | Purpose |
|-----------|------|---------|
| Policy AST | `src/policy/ast.rs` | Abstract syntax tree for policy expressions |
| Policy IR | `src/policy/ir.rs` | Lowered intermediate representation |
| Policy Compiler | `src/policy/mod.rs` | Policy expression compilation |
| Precedence Engine | `src/policy/precedence.rs` | Policy conflict resolution |
| Diagnostics | `src/policy/diagnostics.rs` | Policy validation diagnostics |
| Trace | `src/policy/trace.rs` | Policy evaluation traces |

### Policy Compilation (ADR-024)

The `PolicyCompilerPass` (in `src/compiler/passes/policy.rs`) applies policy during compilation:

- `NodeMetadata` annotations for: retry policy, timeout, approval gates, budget limits
- Declarative policy expressions compiled to internal IR
- Precedence engine resolves conflicts between policies

### Policy Influence Areas

| Area | Application |
|------|-------------|
| Retry | Node retry count, backoff strategy, fallback paths |
| Timeout | Per-node and per-graph timeout enforcement |
| Budget | Token limits, cost ceilings, resource envelopes |
| Approval | Gate nodes requiring human approval |
| Routing | Provider selection preferences |

## Release Governance (`src/release/`)

### Release Gates

8 deterministic gates that must pass for a release:

| Gate | Code | Purpose |
|------|------|---------|
| SDK Version Compatibility | SDK-1 | Plugin API version compatibility check |
| Replay Determinism | REP-1 | Execution replay produces identical results |
| Upgrade Safety | UPG-1 | No breaking changes to runtime ABI |
| Deterministic Compilation | DET-1 | Same WorkflowIR → same ExecutionGraph |
| Plugin Compatibility | PLG-1 | Plugin ABI compatibility |
| Strategy Correctness | STR-1 | Strategy implementations produce correct results |
| Provider Contract | PRO-1 | Provider implementations satisfy contract |
| Connector Stability | CON-1 | Connector implementations pass stability tests |

### Policy Engine (`src/release/policy.rs`)

| Component | File | Purpose |
|-----------|------|---------|
| Policy Evaluator | `src/release/evaluator.rs` | Evaluates policy rules against release |
| Assessment | `src/release/assessment.rs` | Collects and scores gate results |
| Waiver | `src/release/waiver.rs` | Policy waiver management |

### Attestation Subsystem (`src/release/attestation.rs`)

4-phase verification:

1. **Schema** — Attestation document schema validity
2. **Canonical** — Canonical representation verification
3. **Signature** — Cryptographic signature verification
4. **Semantic** — Semantic content verification

### CLI Governance

```
fusion gates list
fusion gates check
fusion gates explain
fusion gates evaluate
fusion gates attest
fusion gates verify-attestation
```

### Environments

| Environment | Policy Level |
|-------------|--------------|
| Development | Advisory gates only |
| Staging | Warning-level enforcement |
| Production | Hard enforcement, all gates required |

### Related ADRs

- ADR-024: Policy compilation into graph metadata
